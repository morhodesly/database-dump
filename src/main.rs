use std::process;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio_postgres::{Client, NoTls};
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "pg-dump", about = "A utility to dump PostgreSQL database tables, users, and roles")]
struct Opt {
    #[structopt(short, long, help = "Database host")]
    host: String,
    
    #[structopt(short = "P", long, help = "Database port", default_value = "5432")]
    port: u16,
    
    #[structopt(short, long, help = "Database name")]
    dbname: String,
    
    #[structopt(short, long, help = "Database user")]
    user: String,
    
    #[structopt(short = "p", long, help = "Database password")]
    password: String,
    
    #[structopt(short, long, help = "Output file (default: stdout)")]
    output: Option<String>,
}

async fn connect(opt: &Opt) -> Result<Client, Box<dyn Error>> {
    let connection_string = format!(
        "host={} port={} dbname={} user={} password={}",
        opt.host, opt.port, opt.dbname, opt.user, opt.password
    );
    
    let (client, connection) = tokio_postgres::connect(&connection_string, NoTls).await?;
    
    // Spawn the connection handler in the background
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("Connection error: {}", e);
        }
    });
    
    Ok(client)
}

async fn connect_with_retry(opt: &Opt, max_retries: u32) -> Result<Client, Box<dyn Error>> {
    let mut retries = 0;
    let mut last_error = None;

    while retries < max_retries {
        match connect(opt).await {
            Ok(client) => return Ok(client),
            Err(e) => {
                eprintln!("Connection attempt {} failed: {}", retries + 1, e);
                last_error = Some(e);
                retries += 1;
                
                if retries < max_retries {
                    // Exponential backoff
                    let delay = Duration::from_secs(2u64.pow(retries.min(4)));
                    eprintln!("Retrying in {} seconds...", delay.as_secs());
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    // If we got here, all retries failed
    Err(match last_error {
        Some(e) => e,
        None => Box::new(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "Failed to connect to the database after retries",
        )),
    })
}

struct DumpTarget<'a> {
    file: Option<&'a mut File>,
}

impl<'a> DumpTarget<'a> {
    fn new(file: Option<&'a mut File>) -> Self {
        DumpTarget { file }
    }
    
    fn write_line(&mut self, line: &str) -> Result<(), Box<dyn Error>> {
        match &mut self.file {
            Some(file) => {
                writeln!(file, "{}", line)?;
            }
            None => {
                println!("{}", line);
            }
        }
        Ok(())
    }
}

async fn dump_tables_to<'a>(client: &Client, target: &'a mut DumpTarget<'a>) -> Result<(), Box<dyn Error>> {
    target.write_line("-- Tables, sequences, data types, and table data")?;
    target.write_line("SET client_encoding = 'UTF8';")?;
    target.write_line("SET standard_conforming_strings = on;")?;
    target.write_line("SET check_function_bodies = false;")?;
    target.write_line("SET client_min_messages = warning;")?;
    target.write_line("SET search_path = public, pg_catalog;")?;
    target.write_line("")?;
    
    // Get and dump custom types first, with better compatibility
    let types = client.query(
        "SELECT t.typname
         FROM pg_catalog.pg_type t 
         JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace
         WHERE t.typtype = 'e'
         AND n.nspname = 'public'
         ORDER BY t.typname",
        &[],
    ).await?;
    
    for type_row in types {
        let type_name: String = type_row.get(0);
        target.write_line(&format!("-- Custom Type: {}", type_name))?;
        
        // Get enum values
        let enum_values = client.query(
            "SELECT e.enumlabel
             FROM pg_catalog.pg_enum e
             JOIN pg_catalog.pg_type t ON e.enumtypid = t.oid
             JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace
             WHERE t.typname = $1
             AND n.nspname = 'public'
             ORDER BY e.enumsortorder NULLS FIRST",
            &[&type_name],
        ).await?;
        
        let mut values = Vec::new();
        for enum_val in enum_values {
            // Use try_get to handle potential errors
            match enum_val.try_get::<_, String>(0) {
                Ok(val) => values.push(format!("'{}'", val)),
                Err(_) => {
                    // Try with &str if String fails
                    if let Ok(val) = enum_val.try_get::<_, &str>(0) {
                        values.push(format!("'{}'", val));
                    }
                    // Skip if we can't get the value
                }
            }
        }
        
        if !values.is_empty() {
            target.write_line(&format!("CREATE TYPE {} AS ENUM ({});", type_name, values.join(", ")))?;
        } else {
            // Log that we couldn't get enum values
            target.write_line(&format!("-- Warning: Could not retrieve enum values for type {}", type_name))?;
        }
        target.write_line("")?;
    }
    
    // Dump sequences
    let sequences = client.query(
        "SELECT c.relname as sequence_name
         FROM pg_catalog.pg_class c
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
         WHERE c.relkind = 'S'
         AND n.nspname = 'public'
         ORDER BY sequence_name",
        &[],
    ).await?;
    
    for seq_row in sequences {
        let seq_name: String = seq_row.get(0);
        target.write_line(&format!("-- Sequence: {}", seq_name))?;
        
        // Get sequence details
        let seq_info = client.query_one(
            "SELECT 
                 pg_catalog.pg_get_expr(d.adbin, d.adrelid) as expression,
                 s.*
             FROM pg_catalog.pg_class c
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
             LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = c.oid
             CROSS JOIN LATERAL pg_catalog.pg_sequence_parameters(c.oid) AS s
             WHERE c.relname = $1
             AND n.nspname = 'public'",
            &[&seq_name],
        ).await?;
        
        // Extract values or use defaults for sequence parameters
        let start_val: i64 = seq_info.try_get(1).unwrap_or(1);
        let min_val: i64 = seq_info.try_get(2).unwrap_or(1);
        let max_val: i64 = seq_info.try_get(3).unwrap_or(2147483647);
        let increment_i64: i64 = seq_info.try_get(4).unwrap_or(1);
        
        target.write_line(&format!("CREATE SEQUENCE {} START WITH {} INCREMENT BY {} MINVALUE {} MAXVALUE {};", 
            seq_name, start_val, increment_i64, min_val, max_val))?;
        target.write_line("")?;
    }
    
    // Get tables - use more reliable pg_catalog queries instead of information_schema
    let tables = client.query(
        "SELECT c.relname as table_name
         FROM pg_catalog.pg_class c
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
         WHERE c.relkind = 'r'
         AND n.nspname = 'public'
         ORDER BY c.relname",
        &[],
    ).await?;
    
    for table_row in tables {
        let table_name: String = table_row.get(0);
        target.write_line(&format!("-- Table: {}", table_name))?;
        
        // Start CREATE TABLE statement
        target.write_line(&format!("CREATE TABLE {} (", table_name))?;
        
        // Get columns using more reliable pg_catalog queries
        let columns_query = client.query(
            "SELECT 
                a.attname as column_name,
                pg_catalog.format_type(a.atttypid, a.atttypmod) as data_type,
                (CASE WHEN a.atttypmod > 0 THEN a.atttypmod - 4 ELSE NULL END) as character_maximum_length,
                a.attnotnull as not_null,
                pg_catalog.pg_get_expr(d.adbin, d.adrelid) as column_default,
                NULL::integer as numeric_precision,
                NULL::integer as numeric_scale
             FROM pg_catalog.pg_attribute a
             LEFT JOIN pg_catalog.pg_attrdef d ON (d.adrelid = a.attrelid AND d.adnum = a.attnum)
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
             WHERE c.relname = $1
             AND n.nspname = 'public'
             AND a.attnum > 0
             AND NOT a.attisdropped
             ORDER BY a.attnum",
            &[&table_name],
        ).await?;
        
        let mut column_defs = Vec::new();
        
        for column in &columns_query {
            let column_name: String = column.get(0);
            let data_type: String = column.get(1);
            let max_length: Option<i32> = column.get(2);
            let not_null: bool = column.get(3);
            let default_val: Option<String> = column.get(4);
            
            // Build column definition
            let mut col_def = format!("  {}", column_name);
            
            // Determine the full type with precision/scale if needed
            if data_type.contains("character varying") && max_length.is_some() {
                col_def.push_str(&format!(" varchar({})", max_length.unwrap()));
            } else if data_type.contains("character") && max_length.is_some() {
                col_def.push_str(&format!(" char({})", max_length.unwrap()));
            } else {
                col_def.push_str(&format!(" {}", data_type));
            }
            
            // Add constraints
            if not_null {
                col_def.push_str(" NOT NULL");
            }
            
            if let Some(def) = default_val {
                col_def.push_str(&format!(" DEFAULT {}", def));
            }
            
            column_defs.push(col_def);
        }
        
        // Add the primary key constraint
        let pk_columns = client.query(
            "SELECT a.attname
             FROM pg_index i
             JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey)
             JOIN pg_class t ON t.oid = i.indrelid
             JOIN pg_namespace n ON n.oid = t.relnamespace
             WHERE t.relname = $1
             AND n.nspname = 'public'
             AND i.indisprimary",
            &[&table_name],
        ).await?;
        
        if !pk_columns.is_empty() {
            let mut pk_cols = Vec::new();
            for pk_col in pk_columns {
                let col_name: String = pk_col.get(0);
                pk_cols.push(col_name);
            }
            
            column_defs.push(format!("  PRIMARY KEY ({})", pk_cols.join(", ")));
        }
        
        // Complete the CREATE TABLE statement
        target.write_line(&column_defs.join(",\n"))?;
        target.write_line(");")?;
        target.write_line("")?;
        
        // Add indexes (excluding primary key which is already created with table)
        let indexes = client.query(
            "SELECT indexname, indexdef FROM pg_indexes 
             WHERE schemaname = 'public' AND tablename = $1
             AND indexname NOT IN (
                 SELECT tc.constraint_name
                 FROM information_schema.table_constraints tc
                 WHERE tc.constraint_type = 'PRIMARY KEY'
                 AND tc.table_schema = 'public'
                 AND tc.table_name = $1
             )",
            &[&table_name],
        ).await?;
        
        for idx in indexes {
            let index_def: String = idx.get(1);
            target.write_line(&format!("{};\n", index_def))?;
        }
        
        // Add foreign key constraints
        let fk_constraints = client.query(
            "SELECT
                 tc.constraint_name,
                 kcu.column_name,
                 ccu.table_name AS foreign_table_name,
                 ccu.column_name AS foreign_column_name,
                 rc.delete_rule,
                 rc.update_rule
             FROM information_schema.table_constraints AS tc
             JOIN information_schema.key_column_usage AS kcu
                 ON tc.constraint_name = kcu.constraint_name
             JOIN information_schema.constraint_column_usage AS ccu
                 ON ccu.constraint_name = tc.constraint_name
             JOIN information_schema.referential_constraints AS rc
                 ON rc.constraint_name = tc.constraint_name
             WHERE tc.constraint_type = 'FOREIGN KEY' 
                 AND tc.table_schema = 'public'
                 AND tc.table_name = $1",
            &[&table_name],
        ).await?;
        
        for fk in fk_constraints {
            let constraint_name: String = fk.get(0);
            let column_name: String = fk.get(1);
            let foreign_table: String = fk.get(2);
            let foreign_column: String = fk.get(3);
            let delete_rule: String = fk.get(4);
            let update_rule: String = fk.get(5);
            
            target.write_line(&format!(
                "ALTER TABLE ONLY {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON UPDATE {} ON DELETE {};",
                table_name, constraint_name, column_name, foreign_table, foreign_column, update_rule, delete_rule
            ))?;
        }
        
        target.write_line("")?;
        
        // Dump table data
        target.write_line(&format!("-- Data for table: {}", table_name))?;
        
        // Get column names for INSERT statements
        let column_names_str = columns_query.iter()
            .map(|col| col.get::<_, String>(0))
            .collect::<Vec<String>>()
            .join(", ");
        
        // Get the data
        let copy_query = format!("SELECT * FROM {}", table_name);
        let rows = client.query(&copy_query, &[]).await?;
        
        // Only proceed if there's data
        if !rows.is_empty() {
            for row in rows {
                let mut values = Vec::new();
                
                for (i, col) in columns_query.iter().enumerate() {
                    let col_type: String = col.get(1);
                    
                    // Try to get value safely with error handling
                    let value = match row.try_get::<_, Option<&str>>(i) {
                        Ok(Some(val)) => {
                            if col_type.contains("char") || col_type == "text" || 
                               col_type.contains("time") || col_type.contains("date") {
                                // String types need quotes and escaping
                                format!("'{}'", val.replace("'", "''"))
                            } else {
                                // Numeric types don't need quotes
                                val.to_string()
                            }
                        },
                        Ok(None) => "NULL".to_string(),
                        Err(_) => {
                            // Try various types when string fails
                            if let Ok(val) = row.try_get::<_, i32>(i) {
                                val.to_string()
                            } else if let Ok(val) = row.try_get::<_, i64>(i) {
                                val.to_string()
                            } else if let Ok(val) = row.try_get::<_, f64>(i) {
                                val.to_string()
                            } else if let Ok(val) = row.try_get::<_, bool>(i) {
                                if val { "TRUE".to_string() } else { "FALSE".to_string() }
                            } else if let Ok(Some(val)) = row.try_get::<_, Option<String>>(i) {
                                format!("'{}'", val.replace("'", "''"))
                            } else {
                                "NULL".to_string()
                            }
                        }
                    };
                    
                    values.push(value);
                }
                
                target.write_line(&format!(
                    "INSERT INTO {} ({}) VALUES ({});",
                    table_name, column_names_str, values.join(", ")
                ))?;
            }
        }
        
        target.write_line("")?;
    }
    
    Ok(())
}

async fn dump_users_and_roles_to<'a>(client: &Client, target: &'a mut DumpTarget<'a>) -> Result<(), Box<dyn Error>> {
    target.write_line("-- Users, roles and permissions")?;
    target.write_line("")?;
    
    // Check if we have access to role-related information
    let has_role_access = client.query_one(
        "SELECT COUNT(*) FROM pg_catalog.pg_roles LIMIT 1",
        &[],
    ).await.is_ok();
    
    if !has_role_access {
        target.write_line("-- Warning: No access to role information. Skipping user and role dump.")?;
        target.write_line("-- You may need superuser privileges to dump roles.")?;
        return Ok(());
    }
    
    let roles = client.query(
        "SELECT r.rolname, r.rolsuper, r.rolinherit, r.rolcreaterole, 
                r.rolcreatedb, r.rolcanlogin, r.rolreplication,
                ARRAY(SELECT b.rolname
                      FROM pg_catalog.pg_auth_members m
                      JOIN pg_catalog.pg_roles b ON (m.roleid = b.oid)
                      WHERE m.member = r.oid) as memberof,
                pg_catalog.shobj_description(r.oid, 'pg_authid') AS description,
                r.rolconnlimit
         FROM pg_catalog.pg_roles r
         ORDER BY r.rolname",
        &[],
    ).await?;
    
    for role in roles {
        let rolname: String = role.get(0);
        // Skip postgres built-in roles
        if rolname.starts_with("pg_") {
            continue;
        }
        
        let is_superuser: bool = role.get(1);
        let inherit: bool = role.get(2);
        let create_role: bool = role.get(3);
        let create_db: bool = role.get(4);
        let can_login: bool = role.get(5);
        let replication: bool = role.get(6);
        let member_of: Vec<String> = role.get(7);
        let description: Option<String> = role.get(8);
        let conn_limit: i32 = role.get(9);
        
        target.write_line(&format!("-- Role: {}", rolname))?;
        
        let mut create_role_stmt = format!("CREATE ROLE {}", rolname);
        
        if is_superuser {
            create_role_stmt.push_str(" SUPERUSER");
        } else {
            create_role_stmt.push_str(" NOSUPERUSER");
        }
        
        if inherit {
            create_role_stmt.push_str(" INHERIT");
        } else {
            create_role_stmt.push_str(" NOINHERIT");
        }
        
        if create_role {
            create_role_stmt.push_str(" CREATEROLE");
        } else {
            create_role_stmt.push_str(" NOCREATEROLE");
        }
        
        if create_db {
            create_role_stmt.push_str(" CREATEDB");
        } else {
            create_role_stmt.push_str(" NOCREATEDB");
        }
        
        if can_login {
            create_role_stmt.push_str(" LOGIN");
        } else {
            create_role_stmt.push_str(" NOLOGIN");
        }
        
        if replication {
            create_role_stmt.push_str(" REPLICATION");
        } else {
            create_role_stmt.push_str(" NOREPLICATION");
        }
        
        if conn_limit >= 0 {
            create_role_stmt.push_str(&format!(" CONNECTION LIMIT {}", conn_limit));
        }
        
        create_role_stmt.push(';');
        target.write_line(&create_role_stmt)?;
        
        // Try to get password if possible (may require superuser)
        let pwd_result = client.query_opt(
            "SELECT rolpassword FROM pg_authid WHERE rolname = $1",
            &[&rolname],
        ).await;
        
        if let Ok(Some(pwd_row)) = pwd_result {
            let pwd: Option<String> = pwd_row.get(0);
            if let Some(password) = pwd {
                if password.starts_with("md5") {
                    target.write_line(&format!("ALTER ROLE {} WITH ENCRYPTED PASSWORD '{}';", rolname, password))?;
                }
            }
        }
        
        // Add to roles if part of any
        if !member_of.is_empty() {
            target.write_line(&format!("GRANT {} TO {};", member_of.join(", "), rolname))?;
        }
        
        // Add description if any
        if let Some(desc) = description {
            target.write_line(
                &format!("COMMENT ON ROLE {} IS '{}';", rolname, desc.replace("'", "''"))
            )?;
        }
        
        target.write_line("")?;
        
        // Get schema level privileges for this role
        let schema_privs = client.query(
            "SELECT n.nspname as schema,
                    array_agg(DISTINCT privilege_type) as privileges
             FROM (
                 SELECT rtg.*, n.nspname as table_schema 
                 FROM information_schema.role_usage_grants rtg
                 JOIN pg_namespace n ON n.nspname = rtg.object_schema
             ) subq
             JOIN pg_namespace n ON n.nspname = subq.table_schema
             WHERE grantee = $1
             GROUP BY n.nspname",
            &[&rolname],
        ).await?;
        
        for sp in schema_privs {
            let schema: String = sp.get(0);
            let privs: Vec<String> = sp.get(1);
            
            target.write_line(&format!(
                "GRANT {} ON SCHEMA {} TO {};", 
                privs.join(", "), schema, rolname
            ))?;
        }
        
        // Get table level privileges
        let table_privs = client.query(
            "SELECT 
                  n.nspname as table_schema, 
                  c.relname as table_name,
                  array_agg(DISTINCT privilege_type) as privileges
             FROM (
                 SELECT rtg.*, n.nspname as table_schema, c.relname as table_name 
                 FROM information_schema.role_table_grants rtg
                 JOIN pg_class c ON c.relname = rtg.table_name
                 JOIN pg_namespace n ON n.oid = c.relnamespace
             ) subq
             JOIN pg_class c ON c.relname = subq.table_name
             JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE grantee = $1
             GROUP BY n.nspname, c.relname",
            &[&rolname],
        ).await?;
        
        for tp in table_privs {
            let schema: String = tp.get(0);
            let table: String = tp.get(1);
            let privs: Vec<String> = tp.get(2);
            
            target.write_line(&format!(
                "GRANT {} ON TABLE {}.{} TO {};", 
                privs.join(", "), schema, table, rolname
            ))?;
        }
        
        target.write_line("")?;
    }
    
    Ok(())
}

async fn run() -> Result<(), Box<dyn Error>> {
    let opt = Opt::from_args();
    
    // Test connection before proceeding with retries
    let client = match connect_with_retry(&opt, 3).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Connection error: {}", e);
            eprintln!("Please check your connection parameters and credentials.");
            return Err(e);
        }
    };
    
    // Test if we can query basic schema information
    let can_query_schema = client.query_one(
        "SELECT COUNT(*) FROM information_schema.tables LIMIT 1",
        &[],
    ).await.is_ok();
    
    if !can_query_schema {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Cannot query database schema. Check your permissions."
        )));
    }
    
    // Determine output: file or stdout
    if let Some(output_file) = &opt.output {
        let mut file = File::create(output_file)?;
        
        // Write headers to file
        writeln!(file, "Database Dump for: {}", opt.dbname)?;
        writeln!(file, "Host: {}:{}\n", opt.host, opt.port)?;
        
        // Create dump target with file
        {
            let mut target = DumpTarget::new(Some(&mut file));
            // Run first dump
            dump_tables_to(&client, &mut target).await?;
        }
        
        {
            let mut target = DumpTarget::new(Some(&mut file));
            // Run second dump
            dump_users_and_roles_to(&client, &mut target).await?;
        }
        
        println!("Dump completed and saved to: {}", output_file);
    } else {
        println!("Database Dump for: {}", opt.dbname);
        println!("Host: {}:{}\n", opt.host, opt.port);
        
        // Create dump target for stdout
        {
            let mut target = DumpTarget::new(None);
            dump_tables_to(&client, &mut target).await?;
        }
        
        {
            let mut target = DumpTarget::new(None);
            dump_users_and_roles_to(&client, &mut target).await?;
        }
    }
    
    Ok(())
}

fn main() {
    let rt = Runtime::new().unwrap();
    if let Err(e) = rt.block_on(run()) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

