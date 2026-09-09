#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactRef(pub(super) [u8; 16]);

impl ArtifactRef {
    pub(super) fn from_vec(bytes: Vec<u8>) -> rusqlite::Result<Self> {
        bytes
            .try_into()
            .map(Self)
            .map_err(|_| rusqlite::Error::InvalidQuery)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryKind {
    FolderScan = 1,
    FolderPlan = 2,
    GoogleTakeoutPlan = 3,
    ApplePhotosPlan = 4,
    PicasaPlan = 5,
}

impl HistoryKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::FolderScan => "Folder scan",
            Self::FolderPlan => "Folder plan",
            Self::GoogleTakeoutPlan => "Google Takeout plan",
            Self::ApplePhotosPlan => "Apple Photos plan",
            Self::PicasaPlan => "Picasa plan",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalStatus {
    Completed = 1,
    Failed = 2,
    Cancelled = 3,
}

impl TerminalStatus {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SafeCounters {
    pub assets: u64,
    pub sidecars: u64,
    pub bytes_read: u64,
    pub warnings: u64,
    pub errors: u64,
    pub max_logical_effects: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRecord {
    pub recorded_unix: i64,
    pub kind: HistoryKind,
    pub status: TerminalStatus,
    pub plan: Option<(ArtifactRef, u32)>,
    pub counters: SafeCounters,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredHistory {
    pub sequence: u64,
    pub record: TerminalRecord,
}

impl StoredHistory {
    pub(super) fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let plan_ref = row.get::<_, Option<Vec<u8>>>(4)?;
        let plan_schema = row.get::<_, Option<u32>>(5)?;
        let plan = match (plan_ref, plan_schema) {
            (Some(bytes), Some(schema)) => Some((ArtifactRef::from_vec(bytes)?, schema)),
            (None, None) => None,
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        Ok(Self {
            sequence: read_u64(row, 0)?,
            record: TerminalRecord {
                recorded_unix: row.get(1)?,
                kind: row.get::<_, i64>(2)?.try_into()?,
                status: row.get::<_, i64>(3)?.try_into()?,
                plan,
                counters: SafeCounters {
                    assets: read_u64(row, 6)?,
                    sidecars: read_u64(row, 7)?,
                    bytes_read: read_u64(row, 8)?,
                    warnings: read_u64(row, 9)?,
                    errors: read_u64(row, 10)?,
                    max_logical_effects: read_u64(row, 11)?,
                },
            },
        })
    }
}

fn read_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(index)?).map_err(|_| rusqlite::Error::InvalidQuery)
}

impl TryFrom<i64> for HistoryKind {
    type Error = rusqlite::Error;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::FolderScan),
            2 => Ok(Self::FolderPlan),
            3 => Ok(Self::GoogleTakeoutPlan),
            4 => Ok(Self::ApplePhotosPlan),
            5 => Ok(Self::PicasaPlan),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

impl TryFrom<i64> for TerminalStatus {
    type Error = rusqlite::Error;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Completed),
            2 => Ok(Self::Failed),
            3 => Ok(Self::Cancelled),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}
