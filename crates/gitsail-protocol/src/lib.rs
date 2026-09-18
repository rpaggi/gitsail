//! Versioned data-transfer objects shared across process boundaries (SAD
//! §14; ADR-008; US-035).
//!
//! Consumers (the CLI's `--json` mode, and eventually VS Code/Desktop) never
//! see `gitsail-domain` types directly: every value crossing this boundary
//! is a DTO from [`dto`], wrapped in a versioned [`Envelope`].

#![forbid(unsafe_code)]

pub mod dto;
pub mod envelope;
pub mod error;
pub mod request_id;

pub use dto::{
    BlameDto, BlameLineDto, BlameOriginDto, BranchDto, BranchKindDto, ChangeTypeDto, CommitDto,
    CommitGraphPageDto, CommitGraphRowDto, DecorationDto, DiffDto, DiffHunkDto, DiffLineDto,
    DiffLineOriginDto, FileChangeDto, FileDiffDto, FileStatusCodeDto, GitTimestampDto,
    GraphEdgeDto, HeadStateDto, RepositoryDto, RepositoryStatusDto, SignatureDto,
};
pub use envelope::{Envelope, Page, SCHEMA_VERSION};
pub use error::ErrorPayload;
pub use request_id::RequestId;
