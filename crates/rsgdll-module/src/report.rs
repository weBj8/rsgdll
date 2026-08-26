use std::any::Any;
use std::error::Error;
use std::fmt;

/// Owned diagnostic for an ordinary Rust callback failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorReport {
    context: &'static str,
    message: String,
    sources: Vec<String>,
    #[cfg(feature = "backtrace")]
    backtrace: Option<String>,
}

impl ErrorReport {
    pub(crate) fn capture(context: &'static str, error: &(dyn Error + 'static)) -> Self {
        let message = error.to_string();
        let mut sources = Vec::new();
        let mut source = error.source();
        while let Some(error) = source {
            sources.push(error.to_string());
            source = error.source();
        }
        Self {
            context,
            message,
            sources,
            #[cfg(feature = "backtrace")]
            backtrace: Some(capture_backtrace()),
        }
    }

    pub(crate) fn message(context: &'static str, message: impl Into<String>) -> Self {
        Self {
            context,
            message: message.into(),
            sources: Vec::new(),
            #[cfg(feature = "backtrace")]
            backtrace: None,
        }
    }

    /// Returns the captured Rust backtrace when diagnostics are enabled.
    #[must_use]
    pub fn backtrace(&self) -> Option<&str> {
        #[cfg(feature = "backtrace")]
        {
            self.backtrace.as_deref()
        }
        #[cfg(not(feature = "backtrace"))]
        {
            None
        }
    }

    pub(crate) fn append(mut self, detail: impl fmt::Display) -> Self {
        use fmt::Write as _;
        let _ = write!(self.message, "; {detail}");
        self
    }
}

impl fmt::Display for ErrorReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.context, self.message)?;
        for source in &self.sources {
            write!(formatter, ": caused by: {source}")?;
        }
        if let Some(backtrace) = self.backtrace() {
            write!(formatter, "\n\nRust backtrace:\n{backtrace}")?;
        }
        Ok(())
    }
}

impl Error for ErrorReport {}

/// Owned diagnostic for a Rust panic caught at the FFI boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicReport {
    context: &'static str,
    message: String,
    #[cfg(feature = "backtrace")]
    backtrace: Option<String>,
}

impl PanicReport {
    pub(crate) fn capture(context: &'static str, payload: Box<dyn Any + Send>) -> Self {
        let message = match payload.downcast::<String>() {
            Ok(message) => *message,
            Err(payload) => match payload.downcast::<&'static str>() {
                Ok(message) => (*message).to_owned(),
                Err(_) => "non-string panic payload".to_owned(),
            },
        };
        Self {
            context,
            message,
            #[cfg(feature = "backtrace")]
            backtrace: Some(capture_backtrace()),
        }
    }

    /// Returns the captured Rust backtrace when diagnostics are enabled.
    #[must_use]
    pub fn backtrace(&self) -> Option<&str> {
        #[cfg(feature = "backtrace")]
        {
            self.backtrace.as_deref()
        }
        #[cfg(not(feature = "backtrace"))]
        {
            None
        }
    }

    pub(crate) fn append(mut self, detail: impl fmt::Display) -> Self {
        use fmt::Write as _;
        let _ = write!(self.message, "; {detail}");
        self
    }
}

impl fmt::Display for PanicReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "panic in {}: {}", self.context, self.message)?;
        if let Some(backtrace) = self.backtrace() {
            write!(formatter, "\n\nRust backtrace:\n{backtrace}")?;
        }
        Ok(())
    }
}

#[cfg(feature = "backtrace")]
fn capture_backtrace() -> String {
    std::backtrace::Backtrace::force_capture().to_string()
}

#[cfg(all(test, feature = "backtrace"))]
mod tests {
    use super::{ErrorReport, PanicReport};

    #[test]
    fn captured_errors_and_panics_include_backtraces() {
        // Given: one ordinary error and one panic payload.
        let error = std::io::Error::other("failure");

        // When: reports capture both failures.
        let error = ErrorReport::capture("module.error", &error);
        let panic = PanicReport::capture("module.panic", Box::new("failure"));

        // Then: both reports retain captured Rust backtraces.
        assert!(error.backtrace().is_some());
        assert!(panic.backtrace().is_some());
    }
}
