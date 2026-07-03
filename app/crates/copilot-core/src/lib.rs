//! Research Paper Copilot core.
//!
//! Owns the `.research` bundle format (the public, versioned contract every
//! later version builds on), the ingestion pipeline, and local search.
//! The Tauri shell is a thin adapter over this crate.

pub mod ai;
pub mod annotations;
#[cfg(feature = "native")]
pub mod arxiv;
pub mod backlinks;
pub mod bundle;
pub mod capabilities;
pub mod chat;
pub mod citations;
pub mod codemap;
pub mod collab;
pub mod concept_registry;
pub mod concepts;
pub mod context;
pub mod contributions;
#[cfg(feature = "native")]
pub mod embeddings;
#[cfg(feature = "native")]
pub mod equations;
pub mod experiments;
pub mod extension;
#[cfg(feature = "native")]
pub mod figures_tables;
pub mod gaps;
#[cfg(feature = "native")]
pub mod graph_index;
pub mod implementations;
pub mod layout;
pub mod learning;
pub mod lessons;
pub mod library;
pub mod novelty;
pub mod objects;
#[cfg(feature = "native")]
pub mod pipeline;
pub mod plugin;
pub mod provider_config;
pub mod registry;
pub mod reproduction;
pub mod reviews;
#[cfg(feature = "native")]
pub mod sandbox;
pub mod schemas;
#[cfg(feature = "native")]
pub mod search;
pub mod sync;
pub mod telemetry;

/// Core crate version, surfaced to the shell for diagnostics.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
