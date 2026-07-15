use std::error::Error;
use std::fmt;

/// Type-erased adapter failure crossing an application port.
#[derive(Debug)]
pub struct PortError {
    source: Box<dyn Error + Send + Sync + 'static>,
}

impl PortError {
    #[must_use]
    pub fn new(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(error),
        }
    }
}

impl fmt::Display for PortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl Error for PortError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}
