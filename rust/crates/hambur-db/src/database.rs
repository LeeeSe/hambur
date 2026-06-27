use crate::*;

impl HamburDatabase {
    pub async fn open(path: impl AsRef<Path>) -> HamburResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                HamburError::Internal(format!("create database directory: {error}"))
            })?;
        }

        let connection = Connection::open(path)?;
        let database = Self { connection };
        database.migrate().await?;
        Ok(database)
    }
}
