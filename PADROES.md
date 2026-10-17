# Padrões de engenharia do agent-pinball

agent-pinball é um painel com tema de pinball para monitorar agentes Codex, Cursor e Claude, escrito em Rust (1.95, edition 2024).
Este documento define os padrões do projeto: toolchain, lint, testes, complexidade, segurança e Docker. O [AGENTS.md](AGENTS.md) é a versão curta para quem está codando.

---

## 1. Toolchain e workspace

- `rust-toolchain.toml` fixa a versão (`channel = "1.95"`, `components = ["rustfmt", "clippy"]`, `profile = "minimal"`). O rustup escolhe essa versão sozinho.
- Workspace Cargo com `resolver = "3"`. O `[workspace.package]` centraliza `version`, `edition`, `rust-version`, `license` e `publish = false`.
- Todas as dependências ficam em `[workspace.dependencies]`, e cada crate usa `dep = { workspace = true }`. As versões ficam num lugar só. Dependências internas por path também declaram `version`, porque sem ela o cargo-deny as conta como wildcard.
- Um crate por responsabilidade. O crate `cli` fica fino: faz parse dos args, carrega a config uma vez, chama funções tipadas da biblioteca e renderiza a saída.
- Crates entram quando têm código real. Nada de crates vazios ou stubs.
- Profiles:
  - `release`: `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`.
  - `dev`: `debug = "line-tables-only"`. Isso reduz o tamanho dos binários de teste e o tempo de link.
  - `dev.package."*"`: `opt-level = 1`. As dependências compilam otimizadas e os testes rodam mais rápido.
  - `dev.build-override`: `opt-level = 3`. Proc-macros e build scripts ficam mais rápidos.
- O core é só Rust: o servidor e a UI compilam apenas com Cargo, sem Node nem bundler. A UI é renderizada no servidor, e uma página pode ter um script vanilla pequeno.

Crates atuais: `pinball-core`, `pinball-web` e `pinball-cli`. Layout alvo:

```
crates/
  pinball-core/       # tipos de domínio: Agent, AgentKind {Codex, Cursor, Claude}, Session, Status
  pinball-collectors/ # leitura dos logs/estado de cada agente (um módulo por provider)
  pinball-store/      # persistência (SQLite, single-writer)
  pinball-web/        # painel (server-rendered, tema de pinball)
  pinball-cli/        # binário fino
  pinball-test-support/
```

---

## 2. Lint

**Lints do workspace** (`Cargo.toml` raiz). Todo crate herda com `[lints] workspace = true`:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
unwrap_used = "warn"
expect_used = "warn"
dbg_macro = "warn"
todo = "warn"
```

Nos módulos de teste, `#[allow(clippy::unwrap_used, clippy::expect_used)]`.

**Warnings viram erro no CI:**
- `RUSTFLAGS: "-D warnings"` no env global do workflow.
- `cargo clippy --workspace --all-targets -- -D warnings`. O `--all-targets` inclui testes, então eles também passam pelo lint.
- `cargo fmt --all -- --check`, com o rustfmt padrão (sem `rustfmt.toml`).
- `git diff --check` pega whitespace sobrando.

**Regras de arquitetura escritas como lint** (`clippy.toml` por crate, com `disallowed-methods`):
- `crates/pinball-web/clippy.toml` proíbe `std::process::Command::new` e as funções de escrita de `std::fs`, porque o painel só lê.
- Cada entrada tem um `reason`, e a mensagem de erro do lint diz o caminho certo.
- O Clippy lê só o `clippy.toml` mais próximo. Um crate com arquivo próprio repete os limites de complexidade da raiz.

**Estilo:**
- Comentário explica o *porquê* e não repete o *quê*. Comentários de código e textos de PR são em inglês.
- Mudanças pequenas e com escopo, sem refactor oportunista nem abstração especulativa.
- Sem código morto nem API pública pela metade.
- `unwrap`, `expect` e `unreachable!` só em testes. Em runtime, use fallback explícito.
- Fronteiras tipadas: IDs, paths e kinds são parseados e normalizados uma vez e depois reusados.
- Sem singletons globais. As dependências são passadas explicitamente.

---

## 3. Testes

**Runner: cargo-nextest**, com aliases em `.cargo/config.toml`:

```toml
[alias]
t  = "nextest run"                          # dia a dia: pula o tier lento
tf = "nextest run --workspace -P full"      # gate: roda tudo
```

**Tiers definidos por nome** (`.config/nextest.toml`):
- Um teste num módulo cujo nome começa com `slow` ou `stress` (`foo::slow::*`) fica de fora do `cargo t`.
- Orçamento: cerca de 1s por teste. O profile padrão marca como lento qualquer teste acima de 5s e mata depois de 2 min.
- `fail-fast = false`: uma execução mostra todas as falhas.
- Profile `ci`: `default-filter = "all()"`, **`retries = 0`** (retry esconde teste flaky), slow-timeout de 60s e kill em 10 min.
- Antes de mover um teste para o tier lento, tente deixá-lo rápido: timeout ou prazo injetável, fixture menor.

**Organização:**
- Testes unitários ficam em `#[cfg(test)] mod tests` no próprio arquivo.
- Testes de integração ficam em `crates/<crate>/tests/suite/*.rs`, declarados em `tests/suite/mod.rs`, que o `lib.rs` inclui sob `#[cfg(test)]`. **Um binário de teste por crate**, porque cada binário custa um link e, no macOS, uma varredura de malware.
- `rstest` para testes parametrizados e `tempfile` para filesystem.
- Doctests rodam à parte com `cargo test --workspace --doc`, porque `--all-targets` não inclui doctests.

**Regras de conteúdo:**
- Todo bugfix ou mudança de comportamento traz um teste de regressão focado.
- Testes de filesystem usam temp dir ou root injetado e nunca a home real.
- **Testes de fronteira de segurança são adversariais e obrigatórios.** Tentam a violação, verificam que ela é recusada e incluem um caso legítimo de controle. O teste precisa *morder*: falhar com o guard removido.
- Quando um default muda, liste todos os consumidores e teste cada um sob o novo default.
- Formatos de terceiros (arquivos e logs de cada agente) são modelados a partir da doc ou da origem do formato, com link no código e um teste para cada caso.
- Cada collector (Codex, Cursor, Claude) é testado contra **fixtures reais anonimizadas**, em `crates/pinball-collectors/fixtures/<provider>/`.
- Parsers de log recebem entrada malformada ou truncada em teste. Nunca podem dar panic.
- O cálculo de status (rodando, parado, travado, erro) é testado de forma table-driven com `rstest`.
- Clock e filesystem são injetados, sem `SystemTime::now()` direto na lógica.

**Pre-push hook** (`scripts/install-git-hooks.sh`): roda fmt, clippy e `cargo tf` antes de todo push e só mexe no próprio bloco marcado no `.git/hooks/pre-push`. Para escapar: `git push --no-verify`.

**CI:** testes em Linux e macOS em todo PR, e um build `--release` separado, porque existem falhas que só aparecem com LTO.

---

## 4. Qualidade ciclomática / complexidade

Os limites são explícitos e aplicados pelo clippy:

```toml
# Cargo.toml (workspace)
[workspace.lints.clippy]
cognitive_complexity = "warn"
too_many_lines = "warn"
too_many_arguments = "warn"
type_complexity = "warn"
```

```toml
# clippy.toml (raiz)
cognitive-complexity-threshold = 15
too-many-lines-threshold = 80
too-many-arguments-threshold = 5
type-complexity-threshold = 250
```

Com `-D warnings` no CI, estourar qualquer um desses limites quebra o build.

Regras:
- `#[allow(...)]` para complexidade só com comentário dizendo *por que* e com escopo na função, nunca no módulo. Mais de 5 argumentos indica que falta uma struct de parâmetros ou um builder.
- Cobertura: `cargo llvm-cov nextest` no CI como relatório, sem gate no início. O gate de 70% em `pinball-core` e `pinball-collectors` entra depois que o código estabilizar.
- Opcional quando o core estabilizar: `cargo mutants` nos parsers dos collectors.

---

## 5. Segurança

**Código:**
- `unsafe_code = "forbid"` no workspace inteiro.
- **Os logs dos agentes contêm segredos e código do usuário.** Todo texto lido de Codex, Cursor ou Claude passa por um sanitizer tipado (`Sanitized<T>`, construído só por `sanitize()`) antes de ser guardado ou renderizado. Nada cru chega ao painel.
- O painel é **read-only** em relação aos agentes: não escreve nos arquivos deles, não envia comando e não mata processo. A regra é aplicada com `disallowed-methods` no crate web, e mudar isso exige auth e confirmação desenhadas antes.
- Paths dos agentes (`~/.codex`, `~/.cursor`, `~/.claude`) vêm de config com root injetável, e os collectors só leem.
- Config é lida uma única vez (`Config::from_lookup`) e passada adiante. O código não lê env espalhado.
- I/O sempre limitado: o head HTTP tem limite de 8 KB (431 acima disso), e cada conexão tem um prazo total de 10s para mandar o pedido, além dos timeouts de escrita.
- Templates com escape automático e `Content-Security-Policy` restritiva quando o painel tiver páginas.

**Dependências:**
- `cargo deny check --all-features` com `deny.toml`:
  - Allowlist de licenças (MIT, Apache-2.0, BSD, ISC, MPL-2.0, Zlib, Unicode-3.0, CC0). GPL e AGPL são rejeitadas.
  - `wildcards = "deny"`, `unknown-registry = "deny"`, `unknown-git = "deny"`, git só com `rev` fixo.
  - Advisories ignorados exigem comentário com justificativa.
- `cargo audit` no CI. Um `--ignore` precisa do id do advisory e de uma justificativa escrita.
- `--locked` nos builds de CI e Docker.
- Antes de adicionar uma dependência, verificar se o projeto já tem essa capacidade.
- Dependabot mensal para GitHub Actions e Cargo.

**Supply chain do CI:**
- **Toda action é fixada por SHA de commit**, com a versão em comentário (`actions/checkout@3d3c42e... # v7.0.1`).
- `permissions: contents: read` no topo do workflow. Escopo extra só por job.
- `concurrency` cancela execuções antigas de PR, mas nunca as de `main`.

**Segredos:**
- gitleaks em todo push e PR e varredura semanal do histórico completo (`secret-scan.yml`).
- `.gitleaks.toml` herda as regras padrão (`useDefault = true`). A allowlist aceita **só regex específicas** de fixtures obviamente falsas (marcador `FAKE`, como `sk-ant-FAKE...`), sem exclusão por path.

**Processo:** `SECURITY.md` com canal privado (GitHub Security Advisory), SLA de resposta (7 dias) e de patch (30 dias), e threat model explícito com o que está e o que não está no escopo.

---

## 6. Docker e localhost

**Postura de rede padrão: loopback.**
- Bind padrão `127.0.0.1:4242`, em constantes num lugar só (`DEFAULT_BIND` em `pinball-cli/src/config.rs`).
- Allowlist do header `Host` (`AGENT_PINBALL_ALLOWED_HOSTS`, padrão `localhost,127.0.0.1,::1`) contra DNS rebinding, checada antes do roteamento: um `Host` fora da lista recebe 403, e um pedido sem `Host` recebe 400.
- **Fail-closed:** o servidor se recusa a subir em endereço não-loopback fora de container. Como ainda não há autenticação, não existe override.
- Quando o painel precisar sair do loopback, isso exige auth antes, e TLS fica a cargo de um reverse proxy.

**Dockerfile multi-stage** (`docker/Dockerfile`):
- `builder`: `rust:1.95-slim-bookworm`, `cargo build --locked --release --bin agent-pinball`, com cache do registry e do target via BuildKit (`--mount=type=cache`).
- `runtime`: `debian:bookworm-slim` com apenas `ca-certificates` e `--no-install-recommends`, apagando `/var/lib/apt/lists/*`.
- **Usuário não-root** (`uid 1000`, shell `nologin`) dono de `/data`.
- `HEALTHCHECK` usando o próprio binário (`agent-pinball status`).
- `ENV AGENT_PINBALL_IN_CONTAINER=1`: dentro do container o bind é `0.0.0.0` (exigência do namespace) e o guard só avisa, em vez de recusar. A exposição real é decidida pelo `-p` no host, que precisa ficar em `127.0.0.1`.
- `.dockerignore` exclui `target/` e `.git/` e, **por último**, os arquivos `.env`, para que nenhuma negação posterior os inclua de novo.

**docker-compose.yml:**
- `ports: "127.0.0.1:4242:4242"`, ou seja, **publica só no loopback do host**.
- `user: "1000:1000"`, volume nomeado para os dados, `restart: unless-stopped`, healthcheck.
- Os diretórios dos agentes (`~/.codex`, `~/.cursor`, `~/.claude`) são montados **`:ro`**, cada um entrando junto com o seu collector. Os dados do próprio painel ficam num volume nomeado separado.
- Segredos ficam em `.env` (gitignored, `required: false`).

**CI:** o job `docker-smoke` monta a imagem e verifica `--version`, que o usuário não é root e que o healthcheck fica healthy.

---

## 7. Gate local antes de abrir PR

Estes são os comandos que o CI também roda:

```bash
cargo fmt --all -- --check
git diff --check
cargo clippy --workspace --all-targets -- -D warnings
cargo tf
cargo test --workspace --doc
cargo deny check --all-features
cargo audit
```

## 8. Onde cada padrão vive

| Arquivo | Conteúdo |
|---|---|
| `rust-toolchain.toml` | versão fixa do Rust |
| `Cargo.toml` | workspace, lints, profiles |
| `clippy.toml` | limites de complexidade |
| `crates/pinball-web/clippy.toml` | `disallowed-methods` (painel só leitura) + limites |
| `.cargo/config.toml` | aliases `t` / `tf` |
| `.config/nextest.toml` | profiles `default` / `full` / `ci` |
| `deny.toml` | política de dependências |
| `.gitleaks.toml` | varredura de segredos |
| `.github/workflows/ci.yml` | fmt, clippy, test, doc, cobertura, deny, audit, gitleaks, release-build, docker-smoke |
| `.github/workflows/secret-scan.yml` | gitleaks semanal no histórico completo |
| `.github/dependabot.yml` | atualização de actions e Cargo |
| `scripts/install-git-hooks.sh` | pre-push com fmt, clippy e `cargo tf` |
| `docker/Dockerfile`, `docker/docker-compose.yml`, `.dockerignore` | seção 6 |
| `SECURITY.md` | threat model |
| `AGENTS.md` + `CLAUDE.md` (`@AGENTS.md`) | regras para os agentes que codam o projeto |
