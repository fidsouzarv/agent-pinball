# Padrões de engenharia do agent-pinball

Referência: [akitaonrails/ai-memory](https://github.com/akitaonrails/ai-memory) (v3.1.1, Rust 1.95, edition 2024).
Este documento registra os padrões que o ai-memory usa e define quais o agent-pinball adota.
Cada seção tem duas partes: **No ai-memory**, que descreve o que existe lá e onde, e **No agent-pinball**, que diz o que vamos seguir.

Contexto do agent-pinball: painel com tema de pinball para monitorar agentes Codex, Cursor e Claude, escrito em Rust.

---

## 1. Toolchain e workspace

### No ai-memory
- `rust-toolchain.toml` fixa a versão (`channel = "1.95"`, `components = ["rustfmt", "clippy"]`, `profile = "minimal"`). O rustup escolhe essa versão sozinho.
- O projeto é um workspace Cargo com `resolver = "3"`. O `[workspace.package]` centraliza `version`, `edition`, `rust-version` e `license`.
- Todas as dependências ficam em `[workspace.dependencies]`, e cada crate usa `dep = { workspace = true }`. As versões ficam num lugar só.
- Um crate por responsabilidade (`core`, `store`, `mcp`, `hooks`, `llm`, `web`, `cli`...). O crate `cli` fica fino: faz parse dos args, carrega a config uma vez, chama funções tipadas da biblioteca e renderiza a saída.
- `ai-memory-test-support` é um crate só de helpers de teste, usado apenas como dev-dependency e nunca publicado.
- Profiles:
  - `release`: `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`.
  - `dev`: `debug = "line-tables-only"`. Isso reduz o tamanho dos binários de teste e o tempo de link.
  - `dev.package."*"`: `opt-level = 1`. As dependências compilam otimizadas e os testes rodam mais rápido.
  - `dev.build-override`: `opt-level = 3`. Proc-macros e build scripts ficam mais rápidos.
- O core é só Rust: o servidor e a UI web compilam apenas com Cargo, sem Node nem bundler. A UI é renderizada no servidor com templates askama, e uma página pode ter um script vanilla pequeno. Qualquer coisa que precise de toolchain JS vai para `companions/`.

### No agent-pinball
Adotamos tudo isso. O bootstrap criou `pinball-core`, `pinball-web` e `pinball-cli`. Os demais crates entram quando tiverem código real, sem crates vazios. Layout alvo:

```
crates/
  pinball-core/       # tipos de domínio: Agent, AgentKind {Codex, Cursor, Claude}, Session, Status
  pinball-collectors/ # leitura dos logs/estado de cada agente (um módulo por provider)
  pinball-store/      # persistência (SQLite, single-writer)
  pinball-web/        # painel (server-rendered, tema de pinball)
  pinball-cli/        # binário fino
  pinball-test-support/
```

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.95"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

---

## 2. Lint

### No ai-memory

**Lints do workspace** (`Cargo.toml` raiz). Todo crate herda com `[lints] workspace = true`:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
# pedantic fica opt-in por crate, quando o código estabilizar
```

**Warnings viram erro no CI:**
- `RUSTFLAGS: "-D warnings"` no env global do workflow.
- `cargo clippy --workspace --all-targets -- -D warnings`. O `--all-targets` inclui testes, então eles também passam pelo lint.
- `cargo fmt --all -- --check`, com o rustfmt padrão. Não existe `rustfmt.toml`.
- `git diff --check` pega whitespace sobrando.

**Regras de arquitetura escritas como lint** (`clippy.toml` por crate, com `disallowed-methods`):
- `crates/ai-memory-web/clippy.toml` proíbe todo método que escreve no wiki, porque a superfície web é read-only por design. Testes que precisam montar fixture usam `#[allow(clippy::disallowed_methods)]`.
- `crates/ai-memory-wiki/clippy.toml` proíbe `std::fs::write`, `rename`, `remove_file` e `copy`, obrigando as escritas a passarem pelo adapter que registra o arquivo para o auto-commit.
- Cada entrada tem um `reason`, e a mensagem de erro do lint diz o caminho certo.

**Estilo (AGENTS.md):**
- Comentário explica o *porquê* e não repete o *quê*.
- Mudanças pequenas e com escopo, sem refactor oportunista nem abstração especulativa.
- Sem código morto nem API pública pela metade.
- `unwrap`, `expect` e `unreachable!` só em testes. Em runtime, use fallback explícito.
- Fronteiras tipadas: IDs, paths e kinds são parseados e normalizados uma vez e depois reusados (newtypes).
- Sem singletons globais nem `lazy_static` de config. As dependências são passadas explicitamente.

### No agent-pinball
Adotamos os lints do workspace como estão e acrescentamos lints que o ai-memory só aplica por convenção:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
unwrap_used = "warn"      # o ai-memory só pede isso por convenção; aqui o lint aplica
expect_used = "warn"
dbg_macro = "warn"
todo = "warn"
```

Nos módulos de teste, `#![allow(clippy::unwrap_used, clippy::expect_used)]`.

Usamos `disallowed-methods` para regras de arquitetura. Exemplo: o crate `pinball-web` não pode chamar métodos de escrita do store nem disparar ações nos agentes. O painel é só leitura.

---

## 3. Testes

### No ai-memory

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
- Antes de mover um teste para o tier lento, a regra é tentar deixá-lo rápido: timeout injetável, fixture menor, SQLite com `journal_mode=MEMORY`.

**Organização:**
- Testes unitários ficam em `#[cfg(test)] mod tests` no próprio arquivo. São cerca de 334 módulos.
- Testes de integração ficam em `crates/<crate>/tests/suite/*.rs`, declarados em `tests/suite/mod.rs`, que é incluído pelo `lib.rs` sob `#[cfg(test)]`. **Um binário de teste por crate.** Cada binário custa um link e, no macOS e no Windows, uma varredura de malware.
- Um teste de layout falha se existir arquivo de teste não declarado ou `tests/*.rs` solto.
- `rstest` para testes parametrizados e `tempfile` para filesystem.
- Doctests rodam à parte com `cargo test --workspace --doc`, porque `--all-targets` não inclui doctests.

**Regras de conteúdo:**
- Todo bugfix ou mudança de comportamento traz um teste de regressão focado.
- Testes de filesystem usam temp dir ou root injetado e nunca a home real.
- Fixtures git não herdam a config da máquina: usam `--no-gpg-sign` e `Signature::now(...)` fixa.
- **Testes de fronteira de segurança são adversariais e obrigatórios.** Tentam a violação, verificam que ela é recusada e incluem um caso legítimo de controle. O teste precisa *morder*: falhar com o guard removido. `docs/security-boundaries.md` mapeia cada guard para o teste que o protege.
- Toda mutação disco+SQL precisa de teste de recovery ou rollback.
- Quando um default muda, liste todos os consumidores e teste cada um sob o novo default.
- Comportamento de terceiros (payloads de hooks de harness) é modelado a partir da doc oficial, com link no código e um teste para cada caso que a doc descreve.
- Concorrência e multi-sessão têm suíte dedicada (`multi_session.rs`), porque teste unitário de uma sessão não enxerga esses bugs.

**Pre-push hook** (`scripts/install-git-hooks.sh`): roda `cargo tf` antes de todo push e só mexe no próprio bloco marcado no `.git/hooks/pre-push`. Para escapar: `git push --no-verify`.

**CI:** Linux em todo PR; macOS com label `full-ci` ou dispatch, obrigatório antes de release; Windows nightly ou com label; cross-build Windows (`cargo xwin`) em todo PR; build `--release` separado, porque falhas que só aparecem com LTO existem.

### No agent-pinball
Adotamos nextest + aliases `t`/`tf` + tiers por nome + `retries = 0` + um binário por crate + pre-push hook.

Específico do domínio:
- Cada collector (Codex, Cursor, Claude) é testado contra **fixtures reais anonimizadas** dos arquivos e logs de cada ferramenta, em `crates/pinball-collectors/fixtures/<provider>/`, com link para a doc ou a origem do formato.
- Parsers de log recebem entrada malformada ou truncada em teste. Nunca podem dar panic.
- O cálculo de status (rodando, parado, travado, erro) é testado de forma table-driven com `rstest`.
- Clock e filesystem são injetados (trait), sem `SystemTime::now()` direto na lógica.

---

## 4. Qualidade ciclomática / complexidade

### No ai-memory
**Não há limite numérico de complexidade configurado.** O que existe:
- Só `clippy::all` está ativo. Desse grupo, o que trata de complexidade é `too_many_arguments` (padrão: 7) e `type_complexity`.
  - `cognitive_complexity` fica no grupo *nursery* e `too_many_lines` no *pedantic*. **Nenhum dos dois está ligado.**
  - Não há `clippy.toml` com `cognitive-complexity-threshold` nem `too-many-lines-threshold`.
- Na prática, `#[allow(clippy::too_many_arguments)]` aparece **79 vezes** no código. O limite existe, mas é contornado com frequência.
- A complexidade é controlada por convenção e revisão: CLI fino, um crate por responsabilidade, fronteiras tipadas, mudanças pequenas e sem abstração especulativa.
- Não há medição de cobertura (tarpaulin ou llvm-cov) nem mutation testing.

### No agent-pinball
Aqui vamos além do ai-memory e deixamos os limites explícitos, aplicados pelo clippy:

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
- Cobertura: `cargo llvm-cov nextest` no CI como relatório (sem gate no início). O gate de 70% em `pinball-core` e `pinball-collectors` entra depois que o código estabilizar.
- Opcional quando o core estabilizar: `cargo mutants` nos parsers dos collectors.

---

## 5. Segurança

### No ai-memory

**Código:**
- `unsafe_code = "forbid"` no workspace inteiro.
- O sanitizer é uma fronteira tipada: `Sanitized<T>` só pode ser construído por `sanitize()`, e é o único caminho de texto não confiável para o store. Ele remove segredos (chaves Anthropic, OpenAI, AWS, GitHub, JWT...) antes de persistir.
- Segredos usam `secrecy`; a comparação de token é em tempo constante (`subtle`); o RNG vem de `getrandom`; senhas usam Argon2id; tokens são SHA-256 com pepper.
- Arquivos novos têm permissão `0600` e diretórios `0700` no Unix, independente do umask.
- Escrita atômica: tmp + rename + fsync.
- Corpo HTTP limitado a 10 MB.
- Operações destrutivas exigem flag de confirmação e verificação de processo vivo (`sysinfo`).
- Config é lida uma única vez (`Config::load()`). O código não lê env espalhado; leitura de env de terceiros passa por lookup injetável.
- Hooks são fire-and-forget e limitados (≤200 ms; 202 ou 429 quando saturado). Não há `tokio::spawn` sem limite nem fila sem limite.

**Dependências:**
- `cargo deny check --all-features` com `deny.toml`:
  - Allowlist de licenças (MIT, Apache-2.0, BSD, ISC, MPL-2.0, Zlib, Unicode-3.0, CC0...). GPL e AGPL são rejeitadas.
  - `wildcards = "deny"`, `unknown-registry = "deny"`, `unknown-git = "deny"`, git só com `rev` fixo.
  - Advisories ignorados exigem comentário com justificativa.
- `cargo audit` no CI, com `--ignore` explícito e justificado.
- `--locked` nos builds de CI e Docker.
- Antes de adicionar uma dependência, verificar se o projeto já tem essa capacidade.
- Dependabot mensal para GitHub Actions.

**Supply chain do CI:**
- **Toda action é fixada por SHA de commit**, com a versão em comentário (`actions/checkout@3d3c42e... # v7.0.1`).
- `permissions: contents: read` no topo do workflow. Escopo extra só por job.
- `concurrency` cancela execuções antigas de PR, mas nunca as de `main`.
- `npm ci --ignore-scripts` nos companions JS.

**Segredos:**
- gitleaks em todo push e PR (diff) e varredura semanal do histórico completo (`secret-scan.yml`).
- `.gitleaks.toml` herda as regras padrão (`useDefault = true`). A allowlist aceita **só regex específicas** de fixtures obviamente falsas (`ghp_FAKE...`, `sk-ant-leak-...`), sem exclusão por path.
- `.gitleaksignore` guarda apenas fingerprints exatas de achados já revisados.

**Processo:** `SECURITY.md` com canal privado (GitHub Security Advisory), SLA de resposta (7 dias) e de patch (30 dias), e threat model explícito com o que está e o que não está no escopo.

### No agent-pinball
Adotamos tudo isso. Pontos específicos de um monitor de agentes:
- **Os logs dos agentes contêm segredos e código do usuário.** Todo texto lido de Codex, Cursor ou Claude passa por um `Sanitized<T>` antes de ser guardado ou renderizado. Nada cru chega ao painel.
- O painel é **read-only** em relação aos agentes: não envia comando nem mata processo, a menos que isso seja decidido explicitamente depois, com auth e confirmação. A regra é aplicada com `disallowed-methods` no crate web.
- Paths dos agentes (`~/.codex`, `~/.cursor`, `~/.claude`) vêm de config com root injetável, e os collectors só leem. Nunca escrevem nesses diretórios.
- Templates com escape automático (askama) e `Content-Security-Policy` restritiva no painel.
- Copiar `deny.toml`, `.gitleaks.toml` (adaptado) e `SECURITY.md` do ai-memory como ponto de partida.

---

## 6. Docker e localhost

### No ai-memory

**Postura de rede padrão: loopback.**
- Bind padrão `127.0.0.1:49374`, em constantes (`DEFAULT_BIND`, `DEFAULT_SERVER_URL`) num lugar só (`config.rs`).
- Allowlist do header `Host` (`AI_MEMORY_ALLOWED_HOSTS`, padrão `localhost,127.0.0.1,::1`) contra DNS rebinding: um `Host` fora da lista recebe 403.
- **Fail-closed:** o servidor se recusa a subir em endereço não-loopback sem token. `--allow-insecure-no-auth` é um override deliberado e com nome assustador.
- TLS fica a cargo de um reverse proxy (Caddy ou cloudflared), com exemplos de compose em `docker/compose.tls.*.yml`. Atrás do proxy, o cookie vira `Secure` por config explícita, sem confiar em `X-Forwarded-Proto`.
- Ao instalar, um token root é gerado e gravado em arquivo `0600`.

**Dockerfile multi-stage** (`docker/Dockerfile`):
- `builder`: `rust:1.95-slim-bookworm`, `cargo build --locked --release -p <cli>`, com cache do registry e do target via BuildKit (`--mount=type=cache`).
- `runtime-base`: `debian:bookworm-slim` com apenas `ca-certificates` e `--no-install-recommends`, apagando `/var/lib/apt/lists/*`.
- **Usuário não-root** (`uid 1000`, shell `nologin`) dono de `/data`, e `USER` definido em cada stage final.
- `HEALTHCHECK` usando o próprio binário (`ai-memory status`).
- `ENV AI_MEMORY_IN_CONTAINER=1`: dentro do container o bind é `0.0.0.0` (exigência do namespace) e o guard só avisa, em vez de recusar. A exposição real é decidida pelo `-p` no host.
- Stages `runtime-prebuilt-*` copiam binários já compilados no CI, sem recompilar Rust dentro do Docker.
- `.dockerignore` exclui `target/`, `.git/`, `node_modules/` e, **por último**, os arquivos de credencial de deploy, para que nenhuma negação posterior os inclua de novo.

**docker-compose.yml:**
- `ports: "127.0.0.1:49374:49374"`, ou seja, **publica só no loopback do host**. Abrir para a LAN é uma mudança explícita.
- `user: "1000:1000"`, volume nomeado para os dados, `restart: unless-stopped`, healthcheck.
- Segredos ficam em `.env` (gitignored, `required: false`), com um `.env.production.example` versionado.

**CI:** o job `docker-smoke` monta a imagem a partir do binário de release e verifica `--version`, a presença dos arquivos empacotados e os subcomandos no `--help`.

### No agent-pinball
Adotamos o mesmo modelo, com um ponto extra:

- Bind padrão `127.0.0.1:<porta>`, allowlist de `Host`, fail-closed fora do loopback sem token.
- Dockerfile multi-stage, `--locked`, runtime slim, usuário não-root, `HEALTHCHECK`, `.dockerignore` com credenciais por último.
- Compose publicando em `127.0.0.1:<porta>:<porta>`.
- **Ponto extra: o container precisa ler os dados dos agentes no host.** Montamos `~/.codex`, `~/.cursor` e `~/.claude` como volumes **`:ro`** (read-only), e o collector também é read-only no código. Os dados do próprio painel ficam num volume nomeado separado.
- Smoke test da imagem no CI, como o `docker-smoke` do ai-memory.

```yaml
# docker/docker-compose.yml (esboço)
services:
  agent-pinball:
    build: { context: .., dockerfile: docker/Dockerfile }
    user: "1000:1000"
    restart: unless-stopped
    ports:
      - "127.0.0.1:4242:4242"
    volumes:
      - pinball-data:/data
      - ${HOME}/.codex:/agents/codex:ro
      - ${HOME}/.cursor:/agents/cursor:ro
      - ${HOME}/.claude:/agents/claude:ro
    env_file: [{ path: .env, required: false }]
volumes:
  pinball-data:
```

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

## 8. Arquivos a criar no bootstrap

| Arquivo | Origem |
|---|---|
| `rust-toolchain.toml` | igual ao do ai-memory |
| `Cargo.toml` (workspace, lints, profiles) | ai-memory + lints extras da seção 2 e da 4 |
| `clippy.toml` | novo (limites de complexidade) |
| `crates/pinball-web/clippy.toml` | padrão `disallowed-methods` do ai-memory |
| `.cargo/config.toml` | aliases `t` / `tf` |
| `.config/nextest.toml` | profiles `default` / `full` / `ci` |
| `deny.toml` | igual ao do ai-memory |
| `.gitleaks.toml` | adaptado |
| `.github/workflows/ci.yml` | fmt, clippy, test, doc, deny, audit, gitleaks, release-build, docker-smoke; actions fixadas por SHA |
| `.github/dependabot.yml` | igual |
| `scripts/install-git-hooks.sh` | pre-push com `cargo tf` |
| `docker/Dockerfile`, `docker/docker-compose.yml`, `.dockerignore` | seção 6 |
| `SECURITY.md` | threat model próprio |
| `AGENTS.md` + `CLAUDE.md` (`@AGENTS.md`) | regras para os agentes que vão codar o projeto |
