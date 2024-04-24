//! Schema operations for the SWITRS sqlite DB creation

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use new_string_template::template::Template;
use rusqlite::{params_from_iter, Connection};
use serde::Deserialize;

/// Specifies which schema and data should be used for creating a table
#[derive(Debug, Deserialize)]
pub struct LookupTable {
    pk_type: String,
    data: PathBuf,
    schema: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct PrimaryTable {
    name: String,
    schema: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct Schema {
    tables: Vec<PrimaryTable>,
    lookup_schema: PathBuf,
    #[serde(alias = "lookup-tables")]
    lookup_tables: HashMap<String, LookupTable>,
}

pub trait NewDB {
    fn connection(&self) -> &Connection;

    /// Create a table where the name and pk_type are passed into the sql as template parameters
    fn create_table(
        &self,
        name: &str,
        pk_type: &str,
        table_schema: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // build the DDL expression
        let ddl = fs::read_to_string(table_schema)?;
        let ddl = Template::new(ddl);
        let data = {
            let mut map = HashMap::new();
            map.insert("table", name);
            map.insert("pk_type", pk_type);
            map
        };

        let ddl = ddl.render(&data)?;

        self.connection().execute_batch(&ddl)?;
        Ok(())
    }

    /// Load data into the named table from the CSV file at the given table_data path
    fn load_data(
        &self,
        name: &str,
        table_data: &Path,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        // open the csv file
        let mut csv = csv::ReaderBuilder::new()
            .quoting(true)
            .has_headers(true)
            .trim(csv::Trim::All)
            .from_path(&table_data)?;

        // build up the insert statement
        let mut field_count = 0;
        let headers_record;
        let (fields, values) = {
            // construct "field = "
            headers_record = csv.headers()?.clone();
            let mut fields = String::new();
            let mut values = String::new();
            let mut first = true;
            for f in &headers_record {
                if !first {
                    fields.push_str(", ");
                    values.push_str(", ");
                } else {
                    first = false;
                }

                fields.push_str(f);
                values.push('?');
                field_count += 1;
            }

            (fields, values)
        };

        if field_count == 0 {
            return Ok(0);
        }

        let insert_stmt = format!("INSERT INTO {name} ({fields}) VALUES({values})");

        let mut stmt = self.connection().prepare(&insert_stmt)?;

        // collect all the data
        let mut count = 0;
        for record in csv.into_records() {
            let record = record?;

            // convert empty strings to NULL, should we change '-' to NULL as well?
            let record_iter = record
                .into_iter()
                .map(|s| if s.is_empty() { None } else { Some(s) });
            stmt.insert(params_from_iter(record_iter))
                .inspect_err(|e| {
                    print!("error on insert: {e}, row: ");
                    for (field, value) in headers_record.iter().zip(record.iter()) {
                        print!("{field}={value},");
                    }
                    println!("");
                })?;
            count += 1;
        }

        Ok(count)
    }

    /// Initialize all the lookup tables in lookup_tables
    fn init_lookup_tables(
        &self,
        lookup_tables: &HashMap<String, LookupTable>,
        table_schema: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (name, table) in lookup_tables {
            eprintln!("LOADING {name}");
            let schema = table.schema.as_deref().unwrap_or(table_schema);
            self.create_table(name, &table.pk_type, schema)?;
            self.load_data(name, &table.data)?;
        }

        Ok(())
    }
}

impl NewDB for Connection {
    fn connection(&self) -> &Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toml() {
        let schemas: Schema = basic_toml::from_str(
            r#"
    tables = [
        {name = "collisions", schema = "schema/collisions.sql" }
    ]

    lookup_schema = "schema/pk_table.sql"

    [lookup-tables]
    beat_type = { pk_type = "CHAR(1)", data = "lookup-tables/BEAT_TYPE.csv" }
"#,
        )
        .expect("failed to read toml");

        assert_eq!(
            schemas.lookup_tables["beat_type"].data,
            Path::new("lookup-tables/BEAT_TYPE.csv")
        );
        assert_eq!(schemas.tables[0].schema, Path::new("schema/collisions.sql"));
    }

    #[test]
    fn test_create_table_char_1() {
        let connection = Connection::open_in_memory().expect("failed to open in memory DB");
        let table = LookupTable {
            pk_type: String::from("CHAR(1)"),
            data: PathBuf::from("lookup-tables/DAY_OF_WEEK.csv"),
            schema: None,
        };

        connection
            .connection()
            .create_table(
                "day_of_week",
                &table.pk_type,
                Path::new("schema/pk_table.sql"),
            )
            .expect("failed to create table");

        connection
            .execute("SELECT * from day_of_week", [])
            .expect("failed to execute query");

        let count = connection
            .connection()
            .load_data("day_of_week", &table.data)
            .expect("failed to create table");

        assert_eq!(7, count);
    }

    #[test]
    fn test_create_table_char_2() {
        let connection = Connection::open_in_memory().expect("failed to open in memory DB");
        let table = LookupTable {
            pk_type: String::from("CHAR(2)"),
            data: PathBuf::from("lookup-tables/PCF_VIOLATION_CATEGORY.csv"),
            schema: None,
        };

        connection
            .connection()
            .create_table(
                "pcf_violation_category",
                &table.pk_type,
                Path::new("schema/pk_table.sql"),
            )
            .expect("failed to create table");

        connection
            .execute("SELECT * from pcf_violation_category", [])
            .expect("failed to execute query");

        let count = connection
            .connection()
            .load_data("pcf_violation_category", &table.data)
            .expect("failed to create table");

        assert_eq!(26, count);
    }

    #[test]
    fn test_create_table_varchar_2() {
        let connection = Connection::open_in_memory().expect("failed to open in memory DB");
        let table = LookupTable {
            pk_type: String::from("VARCHAR2(2)"),
            data: PathBuf::from("lookup-tables/PRIMARY_RAMP.csv"),
            schema: None,
        };

        connection
            .connection()
            .create_table(
                "primary_ramp",
                &table.pk_type,
                Path::new("schema/pk_table.sql"),
            )
            .expect("failed to create table");

        connection
            .execute("SELECT * from primary_ramp", [])
            .expect("failed to execute query");

        let count = connection
            .connection()
            .load_data("primary_ramp", &table.data)
            .expect("failed to create table");

        assert_eq!(10, count);
    }

    #[test]
    fn test_create_collisions() {
        let connection = Connection::open_in_memory().expect("failed to open in memory DB");

        // initialize all the lookup tables
        let schemas: Schema =
            basic_toml::from_slice(&fs::read("Schemas.toml").expect("failed to read toml"))
                .expect("toml is bad");
        connection
            .connection()
            .init_lookup_tables(&schemas.lookup_tables, &schemas.lookup_schema)
            .expect("failed to init lookup tables");

        connection
            .connection()
            .create_table("collisions", "", Path::new("schema/collisions.sql"))
            .expect("failed to create table");

        connection
            .execute("SELECT * from collisions", [])
            .expect("failed to execute query");

        let count = connection
            .connection()
            .load_data("collisions", Path::new("tests/data/collisions.csv"))
            .expect("failed to create table");

        assert_eq!(6, count);
    }

    #[test]
    fn test_create_parties() {
        let connection = Connection::open_in_memory().expect("failed to open in memory DB");

        connection
            .connection()
            .create_table("parties", "", Path::new("schema/parties.sql"))
            .expect("failed to create table");

        connection
            .execute("SELECT * from parties", [])
            .expect("failed to execute query");

        let count = connection
            .connection()
            .load_data("parties", Path::new("tests/data/parties.csv"))
            .expect("failed to create table");

        assert_eq!(11, count);
    }

    #[test]
    fn test_create_victims() {
        let connection = Connection::open_in_memory().expect("failed to open in memory DB");

        connection
            .connection()
            .create_table("victims", "", Path::new("schema/victims.sql"))
            .expect("failed to create table");

        connection
            .execute("SELECT * from victims", [])
            .expect("failed to execute query");

        let count = connection
            .connection()
            .load_data("victims", Path::new("tests/data/victims.csv"))
            .expect("failed to create table");

        assert_eq!(21, count);
    }
}
