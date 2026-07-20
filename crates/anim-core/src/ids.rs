//! Typed ids. Every entity id is a u64 allocated from the project's monotonic
//! counter, so ids are unique across the whole document and stable across
//! sessions (the counter persists).

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            serde::Serialize,
            serde::Deserialize,
        )]
        pub struct $name(pub u64);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}{}", stringify!($name).chars().next().unwrap(), self.0)
            }
        }
    };
}

id_type!(SceneId);
id_type!(CutId);
id_type!(DrawingId);
id_type!(ColumnId);
id_type!(NodeId);
id_type!(LayerId);
id_type!(ImageId);
id_type!(ParamId);
