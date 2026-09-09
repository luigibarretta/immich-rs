use immich_rs_application::{ApplicationError, ApplicationErrorClass};
use immich_rs_client::{ClientError, ClientErrorClass};
use immich_rs_executor::{ExecutorError, ExecutorErrorClass};
use immich_rs_sources::ScanError;

const USAGE_EXIT: u8 = 2;
const SOURCE_EXIT: u8 = 4;
const AUTH_EXIT: u8 = 5;
const COMPATIBILITY_EXIT: u8 = 6;
const NETWORK_EXIT: u8 = 7;
const CHECKPOINT_EXIT: u8 = 8;
const DESTINATION_EXIT: u8 = 9;
const INVARIANT_EXIT: u8 = 70;
const CANCELLED_EXIT: u8 = 130;

pub struct CliFailure {
    exit_code: u8,
    message: String,
}

impl CliFailure {
    pub fn usage(message: &str) -> Self {
        Self::usage_owned(message.to_owned())
    }

    pub const fn usage_owned(message: String) -> Self {
        Self {
            exit_code: USAGE_EXIT,
            message,
        }
    }

    pub fn invariant(message: &str) -> Self {
        Self {
            exit_code: INVARIANT_EXIT,
            message: message.to_owned(),
        }
    }

    pub fn authentication() -> Self {
        Self {
            exit_code: AUTH_EXIT,
            message: "Immich authentication failed".to_owned(),
        }
    }

    pub fn cancelled() -> Self {
        Self {
            exit_code: CANCELLED_EXIT,
            message: "operation cancelled cleanly".to_owned(),
        }
    }

    pub fn from_scan(error: &ScanError) -> Self {
        let exit_code = match error {
            ScanError::Cancelled => CANCELLED_EXIT,
            ScanError::InvalidPlan(_) => INVARIANT_EXIT,
            _ => SOURCE_EXIT,
        };
        Self {
            exit_code,
            message: error.to_string(),
        }
    }

    pub fn from_client(error: ClientError) -> Self {
        let exit_code = match error.class() {
            ClientErrorClass::Authentication => AUTH_EXIT,
            ClientErrorClass::Compatibility => COMPATIBILITY_EXIT,
            ClientErrorClass::Cancelled => CANCELLED_EXIT,
            ClientErrorClass::Protocol => INVARIANT_EXIT,
            _ => NETWORK_EXIT,
        };
        Self {
            exit_code,
            message: error.to_string(),
        }
    }

    pub fn from_executor(error: ExecutorError) -> Self {
        let exit_code = match error.class() {
            ExecutorErrorClass::Cancelled => CANCELLED_EXIT,
            ExecutorErrorClass::Checkpoint => CHECKPOINT_EXIT,
            ExecutorErrorClass::Destination => DESTINATION_EXIT,
            ExecutorErrorClass::SourceChanged
            | ExecutorErrorClass::SourceDiagnostics
            | ExecutorErrorClass::UnsupportedMetadata => SOURCE_EXIT,
            ExecutorErrorClass::Client => NETWORK_EXIT,
            _ => INVARIANT_EXIT,
        };
        Self {
            exit_code,
            message: error.to_string(),
        }
    }

    pub fn from_application(error: &ApplicationError) -> Self {
        let exit_code = match error.class() {
            ApplicationErrorClass::Usage => USAGE_EXIT,
            ApplicationErrorClass::Source => SOURCE_EXIT,
            ApplicationErrorClass::Authentication => AUTH_EXIT,
            ApplicationErrorClass::Compatibility => COMPATIBILITY_EXIT,
            ApplicationErrorClass::Network => NETWORK_EXIT,
            ApplicationErrorClass::Checkpoint => CHECKPOINT_EXIT,
            ApplicationErrorClass::Destination => DESTINATION_EXIT,
            ApplicationErrorClass::Invariant => INVARIANT_EXIT,
            ApplicationErrorClass::Cancelled => CANCELLED_EXIT,
        };
        Self {
            exit_code,
            message: error.to_string(),
        }
    }

    pub const fn exit_code(&self) -> u8 {
        self.exit_code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}
