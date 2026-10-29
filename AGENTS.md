# Working on Romo

- **Commit every time you finish a change.** Once a change builds and its
  checks pass, commit it before you start the next one: one change per
  commit, with a short imperative subject line (for example "Merge pen
  strokes into one path") and a body that says why when it isn't obvious.
- Several agents may work in this tree at once. Stage the files you changed
  by name instead of `git add -A`, so their work-in-progress stays out of
  your commit.
- Before committing, run the checks from the README's Develop section:
  `nix develop -c cargo clippy --all-targets -- -D warnings` and
  `nix develop -c cargo test`.
- Nix builds see only files git tracks, so `git add` new files before
  `nix build`.
