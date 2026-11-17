use std::fmt;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Pointer {
    segments: Vec<String>,
}

impl Pointer {
    pub(crate) fn root() -> Self {
        Self::default()
    }

    pub(crate) fn child(&self, segment: impl ToString) -> Self {
        let mut segments = self.segments.clone();
        segments.push(segment.to_string());
        Self { segments }
    }

    pub(crate) fn from_segments<S: ToString>(segments: impl IntoIterator<Item = S>) -> Self {
        Self {
            segments: segments.into_iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl fmt::Display for Pointer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.segments {
            write!(
                formatter,
                "/{}",
                segment.replace('~', "~0").replace('/', "~1")
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Pointer;

    #[test]
    fn escapes_tilde_and_slash() {
        let pointer = Pointer::root().child("a/b").child("c~d").child(0);
        assert_eq!(pointer.to_string(), "/a~1b/c~0d/0");
        assert_eq!(Pointer::root().to_string(), "");
    }
}
