use std::path::PathBuf;

pub fn config_dir(
    override_dir: Option<PathBuf>,
    default_base: Option<PathBuf>,
    name: &str,
) -> Option<PathBuf> {
    override_dir.or_else(|| default_base.map(|base| base.join(name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn override_and_default() {
        assert_eq!(
            config_dir(Some("/override".into()), Some("/base".into()), "name"),
            Some("/override".into())
        );
        assert_eq!(
            config_dir(None, Some("/base".into()), "name"),
            Some("/base/name".into())
        );
        assert_eq!(config_dir(None, None, "name"), None);
    }
}
