use crate::*;

pub(crate) struct Connection {
    inner: Mutex<rusqlite::Connection>,
}

impl Connection {
    pub(crate) fn open(path: &Path) -> HamburResult<Self> {
        let inner = rusqlite::Connection::open(path).map_err(database_error)?;
        Ok(Self {
            inner: Mutex::new(inner),
        })
    }

    pub(crate) fn execute(
        &self,
        sql: &str,
        params: SqlParams,
    ) -> Result<usize, rusqlite::Error> {
        let result = self
            .inner
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)
            .and_then(|connection| connection.execute(sql, rusqlite::params_from_iter(params)));
        result
    }

    pub(crate) fn execute_batch(&self, sql: &str) -> Result<(), rusqlite::Error> {
        let result = self
            .inner
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)
            .and_then(|connection| connection.execute_batch(sql));
        result
    }

    pub(crate) fn query(
        &self,
        sql: &str,
        params: SqlParams,
    ) -> Result<Rows, rusqlite::Error> {
        let result = self
            .inner
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)
            .and_then(|connection| {
                let mut statement = connection.prepare(sql)?;
                let column_count = statement.column_count();
                let mut rows = statement.query(rusqlite::params_from_iter(params))?;
                let mut values = Vec::new();
                while let Some(row) = rows.next()? {
                    let mut columns = Vec::with_capacity(column_count);
                    for index in 0..column_count {
                        columns.push(row.get::<_, Value>(index)?);
                    }
                    values.push(Row { columns });
                }
                Ok(Rows {
                    rows: values,
                    next_index: 0,
                })
            });
        result
    }
}

pub(crate) type SqlParams = Vec<Value>;

pub(crate) trait IntoSqlValue {
    fn into_sql_value(self) -> Value;
}

impl IntoSqlValue for Value {
    fn into_sql_value(self) -> Value {
        self
    }
}

impl IntoSqlValue for String {
    fn into_sql_value(self) -> Value {
        Value::Text(self)
    }
}

impl IntoSqlValue for &str {
    fn into_sql_value(self) -> Value {
        Value::Text(self.to_string())
    }
}

impl IntoSqlValue for &String {
    fn into_sql_value(self) -> Value {
        Value::Text(self.clone())
    }
}

impl IntoSqlValue for i64 {
    fn into_sql_value(self) -> Value {
        Value::Integer(self)
    }
}

impl IntoSqlValue for i32 {
    fn into_sql_value(self) -> Value {
        Value::Integer(i64::from(self))
    }
}

impl IntoSqlValue for u32 {
    fn into_sql_value(self) -> Value {
        Value::Integer(i64::from(self))
    }
}

impl IntoSqlValue for bool {
    fn into_sql_value(self) -> Value {
        Value::Integer(i64::from(self))
    }
}

pub(crate) struct Rows {
    rows: Vec<Row>,
    next_index: usize,
}

impl Rows {
    pub(crate) fn next(&mut self) -> Result<Option<Row>, rusqlite::Error> {
        let row = self.rows.get(self.next_index).cloned();
        if row.is_some() {
            self.next_index += 1;
        }
        Ok(row)
    }
}

#[derive(Clone)]
pub(crate) struct Row {
    columns: Vec<Value>,
}

impl Row {
    pub(crate) fn get<T>(&self, index: usize) -> Result<T, rusqlite::Error>
    where
        T: rusqlite::types::FromSql,
    {
        let value = self
            .columns
            .get(index)
            .ok_or(rusqlite::Error::InvalidColumnIndex(index))?;
        rusqlite::types::FromSql::column_result(ValueRef::from(value)).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(index, value.data_type(), Box::new(error))
        })
    }
}

pub(crate) fn database_error(error: rusqlite::Error) -> HamburError {
    HamburError::Internal(format!("rusqlite: {error}"))
}
