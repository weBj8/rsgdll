use std::any::Any;
use std::error::Error;
use std::fmt;

/// Owned diagnostic for an ordinary Rust callback failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorReport {
    context: &'static str,
    message: String,
    sources: Vec<String>,
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
        }
    }

    pub(crate) fn message(context: &'static str, message: impl Into<String>) -> Self {
        Self {
            context,
            message: message.into(),
            sources: Vec::new(),
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
        Ok(())
    }
}

impl Error for ErrorReport {}

/// Owned diagnostic for a Rust panic caught at the FFI boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicReport {
    context: &'static str,
    message: String,
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
        Self { context, message }
    }

    pub(crate) fn append(mut self, detail: impl fmt::Display) -> Self {
        use fmt::Write as _;
        let _ = write!(self.message, "; {detail}");
        self
    }
}

impl fmt::Display for PanicReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "panic in {}: {}", self.context, self.message)
    }
}
