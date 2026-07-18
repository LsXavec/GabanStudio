use crate::ids::*;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown scene {0}")]
    UnknownScene(SceneId),
    #[error("unknown cut {0}")]
    UnknownCut(CutId),
    #[error("unknown node {0}")]
    UnknownNode(NodeId),
    #[error("unknown column {0}")]
    UnknownColumn(ColumnId),
    #[error("unknown drawing {0}")]
    UnknownDrawing(DrawingId),
    #[error("unknown cel layer {0}")]
    UnknownLayer(LayerId),
    #[error("connecting {from} -> {to} would create a cycle")]
    WouldCycle { from: NodeId, to: NodeId },
    #[error("pin {pin} out of range for node {node}")]
    PinOutOfRange { node: NodeId, pin: u16 },
    #[error("cut has no output node set")]
    NoOutput,
    #[error("cycle detected during evaluation at node {0}")]
    EvalCycle(NodeId),
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
    #[error("invalid command: {0}")]
    InvalidCommand(String),
    #[error("storage error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("unsupported project schema version {0}")]
    SchemaVersion(i64),
    #[error("corrupt project file: {0}")]
    Corrupt(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;
