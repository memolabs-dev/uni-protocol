//! Command implementations, grouped by what they act on: the contract, the
//! verification, the report, the journal, the authorizations, the handoff
//! artifacts, the pack registry, and the Spec Kit import.
pub mod authorize;
pub mod contract;
pub mod coverage;
pub mod handoff;
pub mod journal;
pub mod pack;
pub mod report;
pub mod speckit;
pub mod verify;
