//! Signature scanning over explicitly provided memory.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

/// Parsed byte signature where `?` and `??` match any byte.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pattern(Vec<Option<u8>>);

impl Pattern {
    /// Finds one unambiguous matching byte offset.
    pub fn find_unique_in(&self, bytes: &[u8]) -> Result<usize, ScanError> {
        let mut matches = bytes
            .windows(self.0.len())
            .enumerate()
            .filter_map(|(offset, window)| {
                self.0
                    .iter()
                    .zip(window)
                    .all(|(expected, actual)| expected.is_none_or(|byte| byte == *actual))
                    .then_some(offset)
            });
        let first = matches.next().ok_or(ScanError::NotFound)?;
        if matches.next().is_some() {
            Err(ScanError::Ambiguous)
        } else {
            Ok(first)
        }
    }
}

/// Signature lookup failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanError {
    /// No byte range matched.
    NotFound,
    /// More than one byte range matched.
    Ambiguous,
}

impl fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotFound => "signature was not found",
            Self::Ambiguous => "signature matched more than once",
        })
    }
}

impl Error for ScanError {}

impl FromStr for Pattern {
    type Err = PatternError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut bytes = Vec::new();
        for (index, token) in value.split_ascii_whitespace().enumerate() {
            if token == "?" || token == "??" {
                bytes.push(None);
            } else if token.len() == 2 {
                match u8::from_str_radix(token, 16) {
                    Ok(byte) => bytes.push(Some(byte)),
                    Err(_) => {
                        return Err(PatternError::InvalidByte {
                            index,
                            token: token.to_owned(),
                        });
                    }
                }
            } else {
                return Err(PatternError::InvalidByte {
                    index,
                    token: token.to_owned(),
                });
            }
        }
        if bytes.is_empty() {
            Err(PatternError::Empty)
        } else {
            Ok(Self(bytes))
        }
    }
}

/// Invalid textual signature.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatternError {
    /// A signature must contain at least one byte or wildcard.
    Empty,
    /// One token was neither a two-digit hex byte nor a wildcard.
    InvalidByte {
        /// Zero-based token position.
        index: usize,
        /// Rejected token.
        token: String,
    },
}

impl fmt::Display for PatternError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("signature pattern is empty"),
            Self::InvalidByte { index, token } => {
                write!(
                    formatter,
                    "invalid signature byte {token:?} at index {index}"
                )
            }
        }
    }
}

impl Error for PatternError {}
