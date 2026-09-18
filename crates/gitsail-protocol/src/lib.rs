//! Versioned data-transfer objects shared across process boundaries (SAD
//! §14; ADR-008; US-035).
//!
//! Consumers (the CLI's `--json` mode, and eventually VS Code/Desktop) never
//! see `gitsail-domain` types directly: every value crossing this boundary
//! is a DTO from [`dto`], wrapped in a versioned [`Envelope`].

#![forbid(unsafe_code)]

pub mod compat;
pub mod dto;
pub mod envelope;
pub mod error;
pub mod request_id;

pub use compat::{parse_envelope, EnvelopeDecodeError, SUPPORTED_SCHEMA_VERSIONS};
pub use dto::{
    AmendPreviewDto, ApplyPatchResultDto, BlameDto, BlameLineDto, BlameOriginDto, BranchDto,
    BranchKindDto, ChangeTypeDto, CherryPickResultDto, CommitDiffDto, CommitDto,
    CommitGraphPageDto, CommitGraphRowDto, CommitResultDto, ConflictSideContentDto,
    ConflictSidesDto, ConflictStageDto, ConflictedFileDto, DecorationDto, DiffDto, DiffHunkDto,
    DiffLineDto, DiffLineOriginDto, FileChangeDto, FileContentDto, FileDiffDto,
    FileStatusCodeDto, ForgeAccountDto, ForgeConnectionStatusDto, ForgeKindDto,
    ForgeLinkTargetDto, GitTimestampDto, GraphEdgeDto, HeadStateDto, InProgressOperationDto,
    LineHistoryDto, LineHistoryEntryDto, LineRangeDto, MergeResultDto, OperationCapabilityDto,
    PatchExportDto, PatchPreviewDto, PullOutcomeDto, PullResultDto, RebaseActionDto,
    RebasePlanDto, RebasePlanEntryDto, RebaseResultDto, RecentRepositoryDto, RemoteDto,
    RepositoryDto, RepositoryStatusDto, RevertResultDto, SignatureDto, SyncTargetDto,
};
pub use envelope::{Envelope, Page, SCHEMA_VERSION};
pub use error::ErrorPayload;
pub use request_id::RequestId;
