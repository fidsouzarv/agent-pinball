#!/usr/bin/env bash
# Installs this repo's pre-push hook. Run once per clone:
#
#   scripts/install-git-hooks.sh
#
# The hook runs the full test tier (`cargo tf`) before a push, because the
# everyday `cargo t` skips the slow/stress tier. Only the marked block is
# managed; anything else in the hook is kept. Bypass a work-in-progress push
# with `git push --no-verify`.

set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"
if git config --get core.hooksPath >/dev/null; then
    echo 'core.hooksPath is set; install the hook in that directory yourself' >&2
    exit 1
fi

hook=$(git rev-parse --git-path hooks/pre-push)
begin="# >>> agent-pinball pre-push >>>"
end="# <<< agent-pinball pre-push <<<"

managed_block=$(cat <<'HOOK'
# >>> agent-pinball pre-push >>>
# Runs the full test tier before a push. See scripts/install-git-hooks.sh.
(
    set -euo pipefail
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    if command -v cargo-nextest >/dev/null 2>&1; then
        cargo nextest run --workspace -P full
    else
        echo "pre-push: nextest not installed, falling back to cargo test"
        cargo test --workspace --all-targets
    fi
)
# The subshell's status is checked here, so a user hook without `set -e`
# cannot let a failing push through.
agent_pinball_status=$?
if [ "$agent_pinball_status" -ne 0 ]; then
    exit "$agent_pinball_status"
fi
unset agent_pinball_status
# <<< agent-pinball pre-push <<<
HOOK
)

mkdir -p "${hook%/*}"
tmp=$(mktemp "${hook}.XXXXXX")
if [[ -s "$hook" ]]; then
    # Drop the previous managed block, keep everything else, append the new one.
    awk -v begin="$begin" -v end="$end" '
        $0 == begin { inside = 1; next }
        $0 == end { inside = 0; next }
        !inside { print }
    ' "$hook" > "$tmp"
    printf '%s\n' "$managed_block" >> "$tmp"
else
    printf '%s\n\n%s\n' '#!/usr/bin/env bash' "$managed_block" > "$tmp"
fi

mv "$tmp" "$hook"
chmod +x "$hook"
echo "installed $hook"
