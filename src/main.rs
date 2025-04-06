use std::process;
use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
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
    
    #[structopt(short, long, help = "Output file (default: <dbname>-dump.sql in dump-output directory)")]
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

async fn dump_schema_to<'a>(client: &Client, target: &'a mut DumpTarget<'a>) -> Result<(), Box<dyn Error>> {
    target.write_line("-- Database schema definition (sequences, types, tables, constraints)  ")?;
    target.write_line("SET client_encoding = 'UTF8';")?;
    target.write_line("SET standard_conforming_strings = on;")?;
    target.write_line("SET check_function_bodies = false;")?;
    target.write_line("SET client_min_messages = warning;")?;
    target.write_line("SET search_path = public, pg_catalog;")?;
    target.write_line("")?;
    
    // Get and dump custom types first
    target.write_line("-- Custom Types")?;
    
    let enum_types = client.query(
        "SELECT t.typname 
         FROM pg_catalog.pg_type t 
         JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace
         WHERE t.typtype = 'e' 
         AND n.nspname = 'public'
         ORDER BY t.typname",
        &[],
    ).await?;
    
    for type_row in enum_types {
        let type_name: String = type_row.get(0);
        
        // Get enum labels
        let enum_values = client.query(
            "SELECT e.enumlabel
             FROM pg_catalog.pg_enum e
             JOIN pg_catalog.pg_type t ON e.enumtypid = t.oid
             WHERE t.typname = $1
             ORDER BY e.enumsortorder",
            &[&type_name],
        ).await?;
        
        if enum_values.is_empty() {
            continue;
        }
        
        let values: Vec<String> = enum_values.iter()
            .filter_map(|row| row.try_get::<_, String>(0).ok())
            .map(|val| format!("'{}'", val.replace("'", "''")))
            .collect();
            
        if !values.is_empty() {
            target.write_line(&format!("CREATE TYPE {} AS ENUM ({});", 
                type_name, values.join(", ")))?;
        }
    }
    
    target.write_line("")?;
    
    // Get and dump sequences
    target.write_line("-- Sequences")?;
    
    let sequences = client.query(
        "SELECT c.relname
         FROM pg_catalog.pg_class c
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
         WHERE c.relkind = 'S'
         AND n.nspname = 'public'
         ORDER BY c.relname",
        &[],
    ).await?;
    
    for seq_row in sequences {
        let seq_name: String = seq_row.get(0);
        target.write_line(&format!("CREATE SEQUENCE {};", seq_name))?;
    }
    
    target.write_line("")?;
    
    // Get table list
    let tables = client.query(
        "SELECT c.relname
         FROM pg_catalog.pg_class c
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
         WHERE c.relkind = 'r' 
         AND n.nspname = 'public'
         ORDER BY c.relname",
        &[],
    ).await?;
    
    // Store table names for later
    let table_names: Vec<String> = tables.iter()
        .map(|row| row.get::<_, String>(0))
        .collect();
    
    // Create tables
    target.write_line("-- Tables")?;
    
    for table_name in &table_names {
        target.write_line(&format!("-- Table: {}", table_name))?;
        
        // Get columns
        let columns = client.query(
            "SELECT 
                a.attname as column_name,
                pg_catalog.format_type(a.atttypid, a.atttypmod) as data_type,
                a.attnotnull as not_null,
                pg_catalog.pg_get_expr(d.adbin, d.adrelid) as column_default
             FROM pg_catalog.pg_attribute a
             LEFT JOIN pg_catalog.pg_attrdef d ON (d.adrelid = a.attrelid AND d.adnum = a.attnum)
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid
             WHERE c.relname = $1
             AND a.attnum > 0
             AND NOT a.attisdropped
             ORDER BY a.attnum",
            &[&table_name],
        ).await?;
        
        target.write_line(&format!("CREATE TABLE {} (", table_name))?;
        
        let mut column_defs = Vec::new();
        
        for column in &columns {
            let column_name: String = column.get(0);
            let data_type: String = column.get(1);
            let not_null: bool = column.get(2);
            let default_val: Option<String> = column.get(3);
            
            let mut col_def = format!("  {}", column_name);
            col_def.push_str(&format!(" {}", data_type));
            
            if not_null {
                col_def.push_str(" NOT NULL");
            }
            
            if let Some(def) = default_val {
                col_def.push_str(&format!(" DEFAULT {}", def));
            }
            
            column_defs.push(col_def);
        }
        
        // Get primary key
        let pk_query = client.query(
            "SELECT a.attname
             FROM pg_catalog.pg_index i
             JOIN pg_catalog.pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey)
             JOIN pg_catalog.pg_class c ON c.oid = i.indrelid
             WHERE c.relname = $1
             AND i.indisprimary",
            &[&table_name],
        ).await?;
        
        if !pk_query.is_empty() {
            let pk_cols: Vec<String> = pk_query.iter()
                .map(|row| row.get::<_, String>(0))
                .collect();
                
            if !pk_cols.is_empty() {
                column_defs.push(format!("  PRIMARY KEY ({})", pk_cols.join(", ")));
            }
        }
        
        target.write_line(&column_defs.join(",\n"))?;
        target.write_line(");")?;
        target.write_line("")?;
    }
    
    // Add indices and constraints
    target.write_line("-- Indexes and constraints")?;
    
    for table_name in &table_names {
        // Add indexes
        let indexes = client.query(
            "SELECT indexdef 
             FROM pg_catalog.pg_indexes 
             WHERE tablename = $1
             AND schemaname = 'public'
             AND indexname NOT LIKE '%_pkey'",
            &[&table_name],
        ).await?;
        
        for idx in indexes {
            let index_def: String = idx.get(0);
            target.write_line(&format!("{};\n", index_def))?;
        }
        
        // Add foreign key constraints
        let fk_constraints = client.query(
            "SELECT
                 conname,
                 pg_catalog.pg_get_constraintdef(oid)
             FROM pg_catalog.pg_constraint
             WHERE conrelid = (
                 SELECT oid FROM pg_catalog.pg_class WHERE relname = $1
                 AND relnamespace = (SELECT oid FROM pg_catalog.pg_namespace WHERE nspname = 'public')
             )
             AND contype = 'f'",
            &[&table_name],
        ).await?;
        
        for fk in fk_constraints {
            let constraint_def: String = fk.get(1);
            target.write_line(&format!("ALTER TABLE {} ADD {};", table_name, constraint_def))?;
        }
    }
    
    // Add table data
    target.write_line("\n-- Table data")?;
    
    for table_name in &table_names {
        target.write_line(&format!("-- Data for table: {}", table_name))?;
        
        // Get column information
        let columns = client.query(
            "SELECT 
                a.attname, 
                pg_catalog.format_type(a.atttypid, a.atttypmod)
             FROM pg_catalog.pg_attribute a
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid
             WHERE c.relname = $1
             AND a.attnum > 0
             AND NOT a.attisdropped
             ORDER BY a.attnum",
            &[&table_name],
        ).await?;
        
        // Only dump data if we have columns
        if columns.is_empty() {
            continue;
        }
        
        // Get column names
        let column_names: Vec<String> = columns.iter()
            .map(|col| col.get::<_, String>(0))
            .collect();
            
        let column_names_str = column_names.join(", ");
        
        // Get table data
        let select_query = format!("SELECT * FROM {}", table_name);
        let rows = client.query(&select_query, &[]).await?;
        
        for row in rows {
            let mut values = Vec::new();
            
            for (i, _) in columns.iter().enumerate() {
                
                // Handle different data types
                let value = if let Ok(Some(val)) = row.try_get::<_, Option<String>>(i) {
                    // String types
                    format!("'{}'", val.replace("'", "''"))
                } else if let Ok(Some(val)) = row.try_get::<_, Option<&str>>(i) {
                    // String types
                    format!("'{}'", val.replace("'", "''"))
                } else if let Ok(val) = row.try_get::<_, i32>(i) {
                    // Integer
                    val.to_string()
                } else if let Ok(val) = row.try_get::<_, i64>(i) {
                    // Big integer
                    val.to_string()
                } else if let Ok(val) = row.try_get::<_, f64>(i) {
                    // Float
                    val.to_string()
                } else if let Ok(val) = row.try_get::<_, bool>(i) {
                    // Boolean
                    if val { "TRUE".to_string() } else { "FALSE".to_string() }
                } else {
                    // NULL or other types
                    "NULL".to_string()
                };
                
                values.push(value);
            }
            
            target.write_line(&format!(
                "INSERT INTO {} ({}) VALUES ({});",
                table_name, column_names_str, values.join(", ")
            ))?;
        }
        
        target.write_line("")?;
    }
    
    Ok(())
}

async fn dump_users_and_roles_to<'a>(client: &Client, target: &'a mut DumpTarget<'a>, db_name: &str) -> Result<(), Box<dyn Error>> {
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
    
    // First get the owner of the database
    let db_owner_query = client.query_one(
        "SELECT r.rolname 
         FROM pg_catalog.pg_database d 
         JOIN pg_catalog.pg_roles r ON d.datdba = r.oid 
         WHERE d.datname = $1",
        &[&db_name],
    ).await;
    
    let mut db_owner = String::new();
    if let Ok(owner_row) = db_owner_query {
        db_owner = owner_row.get(0);
        target.write_line(&format!("-- Database owner: {}", db_owner))?;
    }
    
    // Get the active user too
    let current_user_query = client.query_one("SELECT current_user", &[]).await;
    let mut current_user = String::new();
    if let Ok(user_row) = current_user_query {
        current_user = user_row.get(0);
        target.write_line(&format!("-- Current connection user: {}", current_user))?;
    }
    
    // Get tables in the database to find owners
    let table_owners = client.query(
        "SELECT DISTINCT r.rolname
         FROM pg_catalog.pg_class c
         JOIN pg_catalog.pg_roles r ON c.relowner = r.oid
         JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid
         WHERE c.relkind IN ('r', 'S', 'v')
         AND n.nspname NOT IN ('pg_catalog', 'information_schema')
         AND n.nspname NOT LIKE 'pg_%'",
        &[],
    ).await?;
    
    let mut role_names = Vec::new();
    
    // Always include the database owner and current user
    if !db_owner.is_empty() {
        role_names.push(db_owner.clone());
    }
    
    if !current_user.is_empty() && !role_names.contains(&current_user) {
        role_names.push(current_user.clone());
    }
    
    // Add owners of tables, views, and sequences
    for row in table_owners {
        let role: String = row.get(0);
        if !role_names.contains(&role) {
            role_names.push(role);
        }
    }
    
    // If we have any roles, dump them
    if !role_names.is_empty() {
        target.write_line(&format!("-- Found {} roles associated with this database", role_names.len()))?;
        
        // Get role details
        for role_name in &role_names {
            let role_info = client.query_one(
                "SELECT r.rolname, r.rolsuper, r.rolinherit, r.rolcreaterole, 
                      r.rolcreatedb, r.rolcanlogin, r.rolreplication
                 FROM pg_catalog.pg_roles r
                 WHERE r.rolname = $1",
                &[&role_name],
            ).await?;
            
            let rolname: String = role_info.get(0);
            let is_superuser: bool = role_info.get(1);
            let inherit: bool = role_info.get(2);
            let create_role: bool = role_info.get(3);
            let create_db: bool = role_info.get(4);
            let can_login: bool = role_info.get(5);
            let replication: bool = role_info.get(6);
            
            target.write_line(&format!("-- Role: {} ({})", 
                rolname, 
                if rolname == db_owner {
                    "database owner"
                } else if rolname == current_user {
                    "current user"
                } else {
                    "object owner"
                }
            ))?;
            
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
            
            create_role_stmt.push(';');
            target.write_line(&create_role_stmt)?;
            
            // Try to get password (requires superuser)
            let pwd_result = client.query_opt(
                "SELECT rolpassword FROM pg_catalog.pg_authid WHERE rolname = $1",
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
            
            // Get role memberships involving these roles
            let parent_roles = client.query(
                "SELECT r.rolname
                 FROM pg_catalog.pg_roles r
                 JOIN pg_catalog.pg_auth_members m ON r.oid = m.roleid
                 JOIN pg_catalog.pg_roles ur ON ur.oid = m.member
                 WHERE ur.rolname = $1
                 AND r.rolname = ANY($2)",
                &[&rolname, &role_names],
            ).await?;
            
            for parent in parent_roles {
                let parent_name: String = parent.get(0);
                target.write_line(&format!("GRANT {} TO {};", parent_name, rolname))?;
            }
            
            target.write_line("")?;
        }
    } else {
        target.write_line("-- No roles found that own objects in this database")?;
    }
    
    target.write_line("")?;
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
        "SELECT COUNT(*) FROM pg_catalog.pg_class LIMIT 1",
        &[],
    ).await.is_ok();
    
    if !can_query_schema {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Cannot query database schema. Check your permissions."
        )));
    }
    
    // Create dump-output directory if it doesn't exist
    let dump_dir = Path::new("dump-output");
    if !dump_dir.exists() {
        fs::create_dir(dump_dir)?;
        println!("Created dump-output directory");
    }
    
    // Default output filename or use provided one
    let output_filename = match &opt.output {
        Some(filename) => filename.clone(),
        None => format!("{}-dump.sql", opt.dbname)
    };
    
    // Combine the directory path with the output filename
    let full_path = dump_dir.join(&output_filename);
    
    // Create the output file
    let mut file = File::create(&full_path)?;
    
    // Write headers to file
    writeln!(file, "Database Dump for: {}", opt.dbname)?;
    writeln!(file, "Host: {}:{}\n", opt.host, opt.port)?;
    
    // Create dump target with file
    {
        let mut target = DumpTarget::new(Some(&mut file));
        // First dump users and roles
        dump_users_and_roles_to(&client, &mut target, &opt.dbname).await?;
    }
    
    {
        let mut target = DumpTarget::new(Some(&mut file));
        // Then dump schema (tables, sequences, etc)
        dump_schema_to(&client, &mut target).await?;
    }
    
    println!("Dump completed and saved to: {}", full_path.display());
    
    Ok(())
}

fn main() {
    let rt = Runtime::new().unwrap();
    if let Err(e) = rt.block_on(run()) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

