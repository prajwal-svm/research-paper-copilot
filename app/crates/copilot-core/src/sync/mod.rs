//! Cloud sync (add-cloud-sync): manifests, journal union-merge, E2E
//! encryption, and dumb-blob remotes — the design debt the whole format was
//! structured around, cashed in.
//!
//! Submodules:
//! - [`manifest`] — layer classification per the format table
//! - [`merge`]    — journal set-union (the no-CRDT decision) + LWW/conflicts
//! - [`crypto`]   — XChaCha20-Poly1305 + Argon2id (boring on purpose)
//! - [`remote`]   — the three-verb blob trait + folder backend
//! - [`s3`]       — hand-signed SigV4 over ureq (self-hosted MinIO + R2)
//! - [`engine`]   — the resumable reconcile loop + tombstones

pub mod crypto;
pub mod engine;
pub mod manifest;
pub mod merge;
pub mod remote;
#[cfg(feature = "native")]
pub mod s3;
