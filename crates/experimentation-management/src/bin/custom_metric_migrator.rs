use std::env;
use std::process;
use regex::Regex;
use serde_json::json;
use sqlx::PgPool;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(sqlx::FromRow, Debug, Clone)]
struct CustomMetric {
    metric_id: String,
    name: String,
    custom_sql: Option<String>,
    source_event_type: String,
}

#[derive(Debug, Clone)]
enum MigrationTarget {
    FilteredMean {
        value_column: String,
        filter_sql: String,
    },
    Composite {
        operands: Vec<(String, f64)>,
        operator: String, // "DIVIDE" | "ADD" | "SUBTRACT" | "MULTIPLY" | "WEIGHTED_SUM"
        operator_code: i32,
    },
    WindowedCount {
        event_type: String,
        filter_sql: String,
        window_hours: i32,
    },
    Metricql {
        expression: String,
    },
}

#[tokio::main]
async fn main() {
    // 1. Initialize logging
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "custom_metric_migrator=info".into()),
        )
        .init();

    info!("Starting Kaizen CUSTOM Metric Migrator & Sunset Tool");

    // 2. Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let mut commit = false;
    let mut db_url = None;

    for arg in args.iter().skip(1) {
        if arg == "--commit" {
            commit = true;
        } else if arg.starts_with("--database-url=") {
            db_url = Some(arg.replace("--database-url=", ""));
        }
    }

    let database_url = db_url
        .or_else(|| env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| {
            "postgresql://postgres:postgres@localhost:5432/experimentation".to_string()
        });

    info!(database_url = %database_url, commit = %commit, "Connecting to database");

    let pool = match PgPool::connect(&database_url).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "Failed to connect to PostgreSQL database");
            process::exit(1);
        }
    };

    // 3. Fetch deprecated CUSTOM metrics
    let custom_metrics = match fetch_custom_metrics(&pool).await {
        Ok(metrics) => metrics,
        Err(e) => {
            tracing::error!(error = %e, "Failed to fetch custom metrics");
            process::exit(1);
        }
    };

    if custom_metrics.is_empty() {
        info!("🎉 No deprecated CUSTOM SQL metrics found. Nothing to migrate.");
        return;
    }

    info!(count = %custom_metrics.len(), "Found deprecated CUSTOM SQL metrics");
    println!("\n=== CUSTOM SQL METRIC AUTO-TRANSLATION REPORT ===");
    println!("Mode: {}\n", if commit { "COMMIT (Applying to database)" } else { "DRY-RUN (Side-by-side comparison)" });

    let mut migrated_count = 0;
    let mut failed_count = 0;

    for metric in &custom_metrics {
        let raw_sql = metric.custom_sql.as_deref().unwrap_or("").trim();
        println!("------------------------------------------------------------");
        println!("Metric ID:  \x1b[36m{}\x1b[0m", metric.metric_id);
        println!("Name:       {}", metric.name);
        println!("Raw SQL:    \x1b[33m{}\x1b[0m", raw_sql);

        // Perform translation matching
        match translate_custom_sql(metric) {
            Some(target) => {
                migrated_count += 1;
                print_migration_target(&target);

                if commit {
                    match apply_migration(&pool, &metric.metric_id, &target).await {
                        Ok(_) => println!("\x1b[32m✔ Successfully committed to database.\x1b[0m"),
                        Err(e) => {
                            failed_count += 1;
                            tracing::error!(metric_id = %metric.metric_id, error = %e, "Failed to commit migration");
                        }
                    }
                } else {
                    println!("\x1b[34mℹ Dry-run: Run with --commit to apply this translation.\x1b[0m");
                }
            }
            None => {
                failed_count += 1;
                println!("\x1b[31m⚠ Auto-translation failed: Complex raw SQL, wrapping in fallback MetricQL expression.\x1b[0m");
                let fallback = MigrationTarget::Metricql {
                    expression: format!("mean({}) where {}", 
                        metric.source_event_type, 
                        raw_sql.replace(";", "")
                    )
                };
                print_migration_target(&fallback);
                
                if commit {
                    match apply_migration(&pool, &metric.metric_id, &fallback).await {
                        Ok(_) => println!("\x1b[32m✔ Successfully committed fallback MetricQL wrapper to database.\x1b[0m"),
                        Err(e) => {
                            tracing::error!(metric_id = %metric.metric_id, error = %e, "Failed to commit fallback wrapper");
                        }
                    }
                }
            }
        }
        println!();
    }

    println!("============================================================");
    println!("Migration Summary:");
    println!("  Total Scanned:     {}", custom_metrics.len());
    println!("  Auto-translated:   {}", migrated_count);
    println!("  Fallback Wrapped:  {}", failed_count);
    println!("============================================================");
}

async fn fetch_custom_metrics(pool: &PgPool) -> Result<Vec<CustomMetric>, sqlx::Error> {
    sqlx::query_as::<_, CustomMetric>(
        "SELECT metric_id, name, custom_sql, source_event_type FROM metric_definitions WHERE type = 'CUSTOM'"
    )
    .fetch_all(pool)
    .await
}

fn translate_custom_sql(metric: &CustomMetric) -> Option<MigrationTarget> {
    let raw_sql = metric.custom_sql.as_deref().unwrap_or("").trim();
    let sql = raw_sql.to_lowercase();

    // Pattern 1: Division/Ratio composite metric
    // e.g. "clicks / impressions" or "metric_a / metric_b"
    let ratio_regex = Regex::new(r"^\s*@?([a-z0-9_]+)\s*/\s*@?([a-z0-9_]+)\s*$").unwrap();
    if let Some(caps) = ratio_regex.captures(&sql) {
        return Some(MigrationTarget::Composite {
            operands: vec![
                (caps.get(1).unwrap().as_str().to_string(), 0.0),
                (caps.get(2).unwrap().as_str().to_string(), 0.0),
            ],
            operator: "DIVIDE".to_string(),
            operator_code: 4, // COMPOSITE_OPERATOR_DIVIDE
        });
    }

    // Pattern 2: Addition composite metric
    // e.g. "metric_a + metric_b"
    let add_regex = Regex::new(r"^\s*@?([a-z0-9_]+)\s*\+\s*@?([a-z0-9_]+)\s*$").unwrap();
    if let Some(caps) = add_regex.captures(&sql) {
        return Some(MigrationTarget::Composite {
            operands: vec![
                (caps.get(1).unwrap().as_str().to_string(), 0.0),
                (caps.get(2).unwrap().as_str().to_string(), 0.0),
            ],
            operator: "ADD".to_string(),
            operator_code: 1, // COMPOSITE_OPERATOR_ADD
        });
    }

    // Pattern 3: Filtered Mean via CASE WHEN
    // e.g. "AVG(CASE WHEN platform = 'mobile' THEN duration_ms ELSE NULL END)"
    let case_avg_regex = Regex::new(r"(?s)avg\s*\(\s*case\s+when\s+(.+?)\s+then\s+([a-z0-9_]+)\s+else\s+null\s+end\s*\)").unwrap();
    if let Some(caps) = case_avg_regex.captures(&sql) {
        return Some(MigrationTarget::FilteredMean {
            filter_sql: caps.get(1).unwrap().as_str().trim().to_string(),
            value_column: caps.get(2).unwrap().as_str().trim().to_string(),
        });
    }

    // Pattern 4: Simple Filtered Mean via AVG FILTER
    // e.g. "AVG(duration_ms) FILTER (WHERE platform = 'mobile')"
    let filter_avg_regex = Regex::new(r"(?s)avg\s*\(\s*([a-z0-9_]+)\s*\)\s*filter\s*\(\s*where\s+(.+?)\s*\)").unwrap();
    if let Some(caps) = filter_avg_regex.captures(&sql) {
        return Some(MigrationTarget::FilteredMean {
            value_column: caps.get(1).unwrap().as_str().trim().to_string(),
            filter_sql: caps.get(2).unwrap().as_str().trim().to_string(),
        });
    }

    // Pattern 5: Simple AVG
    // e.g. "AVG(duration_ms)"
    let simple_avg_regex = Regex::new(r"^\s*avg\s*\(\s*([a-z0-9_]+)\s*\)\s*$").unwrap();
    if let Some(caps) = simple_avg_regex.captures(&sql) {
        return Some(MigrationTarget::FilteredMean {
            value_column: caps.get(1).unwrap().as_str().trim().to_string(),
            filter_sql: "1 = 1".to_string(),
        });
    }

    // Pattern 6: Simple COUNT / Windowed Count
    // e.g. "COUNT(1) FILTER (WHERE event_type = 'play')" or similar window counts
    let count_regex = Regex::new(r"(?s)count\s*\(\s*(?:[a-z0-9_]+|1|\*)\s*\)(?:\s+filter\s*\(\s*where\s+(.+?)\s*\))?").unwrap();
    if let Some(caps) = count_regex.captures(&sql) {
        let filter_sql = caps.get(1).map_or("1 = 1".to_string(), |m| m.as_str().trim().to_string());
        
        // Extract event type if specified in filter
        let event_type_regex = Regex::new(r"event_type\s*=\s*'([a-z0-9_]+)'").unwrap();
        let event_type = if let Some(e_caps) = event_type_regex.captures(&filter_sql) {
            e_caps.get(1).unwrap().as_str().to_string()
        } else {
            metric.source_event_type.clone()
        };

        return Some(MigrationTarget::WindowedCount {
            event_type,
            filter_sql,
            window_hours: 24, // Standard default exposure window
        });
    }

    // Tier 2 Fallback: If it starts with a simple select or is an expression, try to compile to MetricQL
    if sql.contains("select") || sql.contains("from") || sql.contains("join") {
        None
    } else {
        // Translate arithmetic or simple combinations to MetricQL
        // e.g. "mean(watch_time) where platform = 'web'"
        Some(MigrationTarget::Metricql {
            expression: sql.replace("filter", "where").replace("avg", "mean"),
        })
    }
}

fn print_migration_target(target: &MigrationTarget) {
    print!("Migration Type: ");
    match target {
        MigrationTarget::FilteredMean { value_column, filter_sql } => {
            println!("\x1b[32mFILTERED_MEAN (Structured)\x1b[0m");
            println!("  Value Column: {}", value_column);
            println!("  Filter SQL:   {}", filter_sql);
            println!("  Compiled Spark SQL Preview:");
            println!("    \x1b[90mSELECT AVG({}) FROM <source_table> WHERE {}\x1b[0m", value_column, filter_sql);
        }
        MigrationTarget::Composite { operands, operator, .. } => {
            println!("\x1b[32mCOMPOSITE (Structured)\x1b[0m");
            println!("  Operator:  {}", operator);
            println!("  Operands:  {:?}", operands);
            println!("  Compiled Spark SQL Preview:");
            let preview = operands.iter()
                .map(|(id, _)| format!("({})", id))
                .collect::<Vec<_>>()
                .join(&format!(" {} ", operator));
            println!("    \x1b[90m{}\x1b[0m", preview);
        }
        MigrationTarget::WindowedCount { event_type, filter_sql, window_hours } => {
            println!("\x1b[32mWINDOWED_COUNT (Structured)\x1b[0m");
            println!("  Event Type:   {}", event_type);
            println!("  Filter SQL:   {}", filter_sql);
            println!("  Window Hours: {}", window_hours);
            println!("  Compiled Spark SQL Preview:");
            println!("    \x1b[90mSELECT COUNT(1) FROM <events> WHERE event_type = '{}' AND {} AND timestamp < exposure + {} hours\x1b[0m", event_type, filter_sql, window_hours);
        }
        MigrationTarget::Metricql { expression } => {
            println!("\x1b[35mMETRICQL (Phase 2 Expression)\x1b[0m");
            println!("  Expression:   \x1b[95m{}\x1b[0m", expression);
        }
    }
}

async fn apply_migration(pool: &PgPool, metric_id: &str, target: &MigrationTarget) -> Result<(), sqlx::Error> {
    match target {
        MigrationTarget::FilteredMean { value_column, filter_sql } => {
            let config_json = json!({
                "value_column": value_column,
                "filter_sql": filter_sql
            });
            sqlx::query(
                "UPDATE metric_definitions 
                 SET type = 'FILTERED_MEAN', type_config = $1, custom_sql = NULL, metricql_expression = NULL 
                 WHERE metric_id = $2"
            )
            .bind(config_json)
            .bind(metric_id)
            .execute(pool)
            .await?;
        }
        MigrationTarget::Composite { operands, operator_code, .. } => {
            let config_json = json!({
                "operator": operator_code,
                "operands": operands.iter().map(|(id, w)| json!({ "metric_id": id, "weight": w })).collect::<Vec<_>>()
            });
            sqlx::query(
                "UPDATE metric_definitions 
                 SET type = 'COMPOSITE', type_config = $1, custom_sql = NULL, metricql_expression = NULL 
                 WHERE metric_id = $2"
            )
            .bind(config_json)
            .bind(metric_id)
            .execute(pool)
            .await?;
        }
        MigrationTarget::WindowedCount { event_type, filter_sql, window_hours } => {
            let config_json = json!({
                "event_type": event_type,
                "filter_sql": filter_sql,
                "window_hours": window_hours
            });
            sqlx::query(
                "UPDATE metric_definitions 
                 SET type = 'WINDOWED_COUNT', type_config = $1, custom_sql = NULL, metricql_expression = NULL 
                 WHERE metric_id = $2"
            )
            .bind(config_json)
            .bind(metric_id)
            .execute(pool)
            .await?;
        }
        MigrationTarget::Metricql { expression } => {
            sqlx::query(
                "UPDATE metric_definitions 
                 SET type = 'METRICQL', metricql_expression = $1, type_config = NULL, custom_sql = NULL 
                 WHERE metric_id = $2"
            )
            .bind(expression)
            .bind(metric_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}
