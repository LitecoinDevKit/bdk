# Keys: at-rest blobs are context-bound; this crate does not hold the KDF

## Decision

Apps supply a 32-byte key. New seals use the v2 `MWEBSEAL` envelope with `SealContext` (Coins / SyncState / Index / History) bound as AEAD AAD. Opening under the wrong context fails. `seal` still writes legacy `nonce || ct` so old binaries can share a directory; `open` reads both. Rollback detection is a caller-supplied counter (`open_with_context_at_least`); the crate does not store the high-water mark.

## Why

`crates/mweb/src/encrypt.rs` module docs: without AAD, `mweb_history.enc` can be renamed over `mweb_coins.enc` and authentication still passes; restoring an older copy is likewise undetectable. v2 binds magic/version/context/counter so those edits fail the tag. BDK does not manage KDF or key storage.

## How we'd know it broke

- `wrong_context_is_rejected` and the coins↔history swap case in `crates/mweb/src/encrypt.rs`.
- Bit-flip / context-byte edit / truncation tests in the same module — must error, never return plaintext.
- `open_changeset_with_context` rejects `SealContext::History` on a Coins blob and a counter below the caller's minimum.
- Fuzz target `crates/mweb/fuzz/fuzz_targets/open_sealed.rs` — wrong context and low counter fail closed.
- `seal` still writing legacy is load-bearing for rollout (`F-09d` in `docs/SECURITY_PLAN.md`); a silent flip to v2-only `seal` breaks mixed-version directories, caught by the “legacy blob opens / v2 rejected by legacy-only reader” tests in `encrypt.rs`.
