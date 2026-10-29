//! The commands on saved connections, for agents without MCP: the same tools as
//! `platypus-mcp`, with the same agent access, printing one JSON document on stdout.
//! A failure prints `{"error": {"kind": …, "message": …}}` and exits with 1.

use std::process::ExitCode;
use std::sync::Arc;

use platypus_mcp::{Agent, AgentError, Format, HttpLink, Profile, QueryArgs as AgentQuery};
use serde_json::{Value, json};

use crate::{Cli, Command, OutputFormat, QueryArgs};

/// The CLI's name in the query history.
const ORIGIN: &str = "platypus-cli";

enum Output {
    Json(Value),
    Text(String),
}

pub async fn main(cli: Cli) -> ExitCode {
    match run(cli).await {
        Ok(Output::Json(doc)) => {
            println!("{doc}");
            ExitCode::SUCCESS
        }
        Ok(Output::Text(text)) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{}", e.to_json());
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<Output, AgentError> {
    let profile = Profile::from_env()?;
    // The open app asks the user before a change.
    let link = Arc::new(HttpLink::new(profile.data_dir()));
    let agent = Agent::new(profile).with_link(link);
    let named = cli.connection.as_deref().ok_or_else(|| {
        AgentError::Invalid("--connection names the saved connection to use".to_owned())
    });
    let json = Output::Json;
    Ok(match cli.command {
        Command::Connections => json(json!(agent.list_connections()?)),
        Command::Schemas => json(agent.list_schemas(named?).await?),
        Command::Objects {
            schema,
            offset,
            limit,
        } => json(agent.list_objects(named?, &schema, offset, limit).await?),
        Command::Table { schema, table } => {
            json(agent.describe_table(named?, &schema, &table).await?)
        }
        Command::Ddl { schema, name, kind } => {
            let ddl = agent
                .object_ddl(named?, &schema, &name, kind.as_deref())
                .await?;
            json(json!({ "ddl": ddl }))
        }
        Command::Search { text } => json(agent.search_objects(named?, &text).await?),
        Command::Query(args) => query(&agent, named?, args).await?,
    })
}

async fn query(agent: &Agent, connection: &str, args: QueryArgs) -> Result<Output, AgentError> {
    if args.url.is_some() || args.output.is_some() || args.stats {
        return Err(AgentError::Invalid(
            "with --connection, query takes no --url, --output or --stats".to_owned(),
        ));
    }
    let format = match args.format {
        None | Some(OutputFormat::Json) => Format::Json,
        Some(OutputFormat::Table) => Format::Text,
        Some(_) => {
            return Err(AgentError::Invalid(
                "with --connection, the format is json or table".to_owned(),
            ));
        }
    };
    let request = AgentQuery {
        connection: connection.to_owned(),
        sql: crate::read_sql(args.sql).map_err(|e| AgentError::Invalid(e.to_string()))?,
        max_rows: args.max_rows.map(|n| u64::try_from(n).unwrap_or(u64::MAX)),
        max_bytes: args.max_bytes,
        timeout_seconds: args.timeout,
        format: Some(format),
    };
    let result = agent.run_query(&request, ORIGIN).await?;
    Ok(match format {
        Format::Json => Output::Json(result.json()),
        Format::Text => Output::Text(result.text()),
    })
}
