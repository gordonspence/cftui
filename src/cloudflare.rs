use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Timelike, Utc};
use reqwest::blocking::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration as Timeout;

#[derive(Clone, Debug, Default, Serialize)]
pub struct MetricRow {
    pub name: String,
    pub first: u64,
    pub second: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub fetched_at: String,
    pub workers: Option<Vec<MetricRow>>,
    pub databases: Option<Vec<MetricRow>>,
    pub worker_history: Option<History>,
    pub worker_histories: Option<BTreeMap<String, History>>,
    pub database_history: Option<History>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct History {
    pub start: DateTime<Utc>,
    pub first: Vec<u64>,
    pub second: Vec<u64>,
}

fn history_end() -> DateTime<Utc> {
    Utc::now()
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap()
        .with_nanosecond(0)
        .unwrap()
}

impl Snapshot {
    pub fn demo() -> Self {
        Self {
            fetched_at: Utc::now().format("%H:%M:%S UTC").to_string(),
            workers: Some(vec![
                MetricRow {
                    name: "api-production".into(),
                    first: 128_430,
                    second: 42,
                },
                MetricRow {
                    name: "public-site".into(),
                    first: 47_820,
                    second: 3,
                },
                MetricRow {
                    name: "image-delivery".into(),
                    first: 14_320,
                    second: 0,
                },
                MetricRow {
                    name: "webhooks".into(),
                    first: 8_980,
                    second: 0,
                },
                MetricRow {
                    name: "scheduled-jobs".into(),
                    first: 5_780,
                    second: 0,
                },
                MetricRow {
                    name: "redirects".into(),
                    first: 3_920,
                    second: 0,
                },
                MetricRow {
                    name: "email-handler".into(),
                    first: 980,
                    second: 0,
                },
                MetricRow {
                    name: "preview-api".into(),
                    first: 390,
                    second: 0,
                },
            ]),
            databases: Some(vec![
                MetricRow {
                    name: "demo-app-db".into(),
                    first: 2_841_910,
                    second: 19_320,
                },
                MetricRow {
                    name: "demo-analytics-db".into(),
                    first: 932_000,
                    second: 84_100,
                },
            ]),
            worker_history: Some(History {
                start: history_end() - Duration::hours(24),
                first: vec![
                    1800, 1600, 1400, 1100, 900, 1100, 1800, 3200, 6100, 9800, 14200, 18100, 15900,
                    11400, 9200, 7600, 12300, 17500, 14900, 11300, 9000, 7700, 5200, 3900,
                ],
                second: vec![
                    0, 0, 1, 0, 0, 0, 1, 1, 2, 1, 4, 5, 3, 2, 1, 1, 2, 6, 5, 2, 1, 1, 2, 3,
                ],
            }),
            worker_histories: Some(
                [
                    ("api-production", 9u64),
                    ("public-site", 3),
                    ("image-delivery", 1),
                    ("webhooks", 1),
                    ("scheduled-jobs", 1),
                    ("redirects", 1),
                    ("email-handler", 1),
                    ("preview-api", 1),
                ]
                .into_iter()
                .map(|(name, scale)| {
                    (
                        name.into(),
                        History {
                            start: history_end() - Duration::hours(24),
                            first: (0..24)
                                .map(|h| (500 + ((h * 137) % 1100)) * scale)
                                .collect(),
                            second: (0..24)
                                .map(|h| {
                                    if name == "api-production" && h % 5 == 0 {
                                        3
                                    } else {
                                        0
                                    }
                                })
                                .collect(),
                        },
                    )
                })
                .collect(),
            ),
            database_history: Some(History {
                start: history_end() - Duration::hours(24),
                first: vec![
                    40000, 32000, 28000, 22000, 18000, 20000, 31000, 68000, 130000, 220000, 310000,
                    400000, 280000, 180000, 160000, 140000, 230000, 390000, 310000, 220000, 180000,
                    130000, 110000, 70000,
                ],
                second: vec![
                    800, 600, 400, 300, 300, 400, 600, 1200, 2600, 4800, 6200, 8500, 7200, 5500,
                    4000, 3800, 5400, 7900, 6100, 4200, 3800, 3100, 2300, 1700,
                ],
            }),
            warnings: vec![],
        }
    }
}

pub struct Cloudflare {
    client: Client,
    account: String,
    token: String,
}
impl Cloudflare {
    pub fn new(account: String, token: String) -> Result<Self> {
        Ok(Self {
            client: Client::builder().timeout(Timeout::from_secs(20)).build()?,
            account,
            token,
        })
    }
    fn query(&self, dataset: &str, fields: &str, dimension: &str) -> Result<Vec<MetricRow>> {
        // Only group by resource, avoiding hour/status buckets and percentile aggregation.
        let query = format!("query Metrics($account: string!, $start: string!, $end: string!) {{ viewer {{ accounts(filter: {{accountTag: $account}}) {{ {dataset}(limit: 10000, filter: {{datetime_geq: $start, datetime_leq: $end}}) {{ sum {{ {fields} }} dimensions {{ {dimension} }} }} }} }} }}");
        let body = self.request(query, Utc::now())?;
        parse_rows(&body, dataset, fields, dimension)
    }
    fn request(&self, query: String, end: DateTime<Utc>) -> Result<Value> {
        let response = self.client.post("https://api.cloudflare.com/client/v4/graphql")
            .bearer_auth(&self.token).json(&json!({"query":query,"variables":{"account":self.account,"start":(end-Duration::hours(24)).to_rfc3339(),"end":end.to_rfc3339()}})).send().context("Cloudflare connection failed")?;
        let status = response.status();
        if !status.is_success() {
            bail!("Cloudflare HTTP {status}; check token permissions and account ID");
        }
        let body: Value = response
            .json()
            .context("Invalid Cloudflare JSON response")?;
        Ok(body)
    }
    fn history(&self, dataset: &str, fields: &str) -> Result<History> {
        let end = history_end();
        let query = format!("query History($account: string!, $start: string!, $end: string!) {{ viewer {{ accounts(filter: {{accountTag: $account}}) {{ {dataset}(limit: 10000, filter: {{datetime_geq: $start, datetime_lt: $end}}) {{ sum {{ {fields} }} dimensions {{ datetimeHour }} }} }} }} }}");
        let body = self.request(query, end)?;
        parse_history(&body, dataset, fields, end - Duration::hours(24))
    }
    fn worker_histories(&self) -> Result<(History, BTreeMap<String, History>)> {
        let end = history_end();
        let query = "query History($account: string!, $start: string!, $end: string!) { viewer { accounts(filter: {accountTag: $account}) { workersInvocationsAdaptive(limit: 10000, filter: {datetime_geq: $start, datetime_lt: $end}) { sum { requests errors } dimensions { datetimeHour scriptName } } } } }";
        let body = self.request(query.into(), end)?;
        parse_worker_histories(&body, end - Duration::hours(24))
    }
    pub fn fetch(&self) -> Snapshot {
        let mut warnings = vec![];
        let mut get =
            |dataset, fields, dimension, label| match self.query(dataset, fields, dimension) {
                Ok(rows) => Some(rows),
                Err(e) => {
                    warnings.push(format!("{label}: {e}"));
                    None
                }
            };
        let workers = get(
            "workersInvocationsAdaptive",
            "requests errors",
            "scriptName",
            "Workers",
        );
        let databases = get(
            "d1AnalyticsAdaptiveGroups",
            "rowsRead rowsWritten",
            "databaseId",
            "D1",
        );
        let mut history = |dataset, fields, label| match self.history(dataset, fields) {
            Ok(history) => Some(history),
            Err(e) => {
                warnings.push(format!("{label} history: {e}"));
                None
            }
        };
        let database_history = history("d1AnalyticsAdaptiveGroups", "rowsRead rowsWritten", "D1");
        let (worker_history, worker_histories) = match self.worker_histories() {
            Ok((total, individual)) => (Some(total), Some(individual)),
            Err(e) => {
                warnings.push(format!("Workers history: {e}"));
                (None, None)
            }
        };
        Snapshot {
            fetched_at: Utc::now().format("%H:%M:%S UTC").to_string(),
            workers,
            databases,
            worker_history,
            worker_histories,
            database_history,
            warnings,
        }
    }
}

fn parse_worker_histories(
    body: &Value,
    start: DateTime<Utc>,
) -> Result<(History, BTreeMap<String, History>)> {
    let rows = dataset_rows(body, "workersInvocationsAdaptive")?;
    let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for row in rows {
        let name = row["dimensions"]["scriptName"]
            .as_str()
            .context("Missing Worker name in history")?;
        groups.entry(name.into()).or_default().push(row.clone());
    }
    let mut individual = BTreeMap::new();
    for (name, rows) in groups {
        let body = json!({"data":{"viewer":{"accounts":[{"series":rows}]}}});
        individual.insert(
            name,
            parse_history(&body, "series", "requests errors", start)?,
        );
    }
    Ok((
        parse_history(body, "workersInvocationsAdaptive", "requests errors", start)?,
        individual,
    ))
}

fn parse_rows(
    body: &Value,
    dataset: &str,
    fields: &str,
    dimension: &str,
) -> Result<Vec<MetricRow>> {
    let rows = dataset_rows(body, dataset)?;
    let fields: Vec<_> = fields.split_whitespace().collect();
    let mut result = rows
        .iter()
        .map(|row| -> Result<MetricRow> {
            Ok(MetricRow {
                name: row["dimensions"][dimension]
                    .as_str()
                    .context("Missing resource identifier")?
                    .into(),
                first: row["sum"][fields[0]]
                    .as_u64()
                    .context("Missing primary metric")?,
                second: row["sum"][fields[1]]
                    .as_u64()
                    .context("Missing secondary metric")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    result.sort_by_key(|row| std::cmp::Reverse(row.first));
    Ok(result)
}

fn dataset_rows<'a>(body: &'a Value, dataset: &str) -> Result<&'a Vec<Value>> {
    if body["errors"].as_array().is_some_and(|e| !e.is_empty()) {
        // Do not echo provider payloads: they may contain query details.
        bail!("Analytics query rejected; check Analytics Read permission and dataset access");
    }
    let accounts = body
        .pointer("/data/viewer/accounts")
        .and_then(Value::as_array)
        .context("Missing account data")?;
    let account = accounts
        .first()
        .context("Account not accessible with this token")?;
    let rows = account[dataset].as_array().context("Dataset unavailable")?;
    if rows.len() >= 10000 {
        bail!("Result limit reached; refusing to show incomplete totals");
    }
    Ok(rows)
}

fn parse_history(
    body: &Value,
    dataset: &str,
    fields: &str,
    start: DateTime<Utc>,
) -> Result<History> {
    let rows = dataset_rows(body, dataset)?;
    let fields: Vec<_> = fields.split_whitespace().collect();
    let mut history = History {
        start,
        first: vec![0; 24],
        second: vec![0; 24],
    };
    for row in rows {
        let timestamp = row["dimensions"]["datetimeHour"]
            .as_str()
            .context("Missing hour bucket")?;
        let hour = DateTime::parse_from_rfc3339(timestamp)
            .context("Invalid hour bucket")?
            .with_timezone(&Utc);
        let seconds = (hour - start).num_seconds();
        if !(0..86400).contains(&seconds) || seconds % 3600 != 0 {
            bail!("Unexpected hour bucket; history unavailable");
        }
        let index = (seconds / 3600) as usize;
        for (values, field) in [
            (&mut history.first, fields[0]),
            (&mut history.second, fields[1]),
        ] {
            let value = row["sum"][field]
                .as_u64()
                .context("Missing history metric")?;
            values[index] = values[index]
                .checked_add(value)
                .context("History metric overflow")?;
        }
    }
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_history_keeps_each_script_separate() {
        let start = DateTime::parse_from_rfc3339("2026-09-25T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let body = json!({"data":{"viewer":{"accounts":[{"workersInvocationsAdaptive":[
            {"dimensions":{"scriptName":"a","datetimeHour":"2026-09-25T12:00:00Z"},"sum":{"requests":10,"errors":1}},
            {"dimensions":{"scriptName":"b","datetimeHour":"2026-09-25T12:00:00Z"},"sum":{"requests":90,"errors":3}},
            {"dimensions":{"scriptName":"a","datetimeHour":"2026-09-25T12:00:00Z"},"sum":{"requests":5,"errors":0}}
        ]}]}}});
        let (total, workers) = parse_worker_histories(&body, start).unwrap();
        assert_eq!(total.first[0], 105);
        assert_eq!(workers["a"].first[0], 15);
        assert_eq!(workers["b"].first[0], 90);
        assert_eq!(workers["a"].second[0], 1);
        assert_eq!(workers["b"].second[0], 3);
        assert_eq!(workers["a"].first[1], 0);
    }
    #[test]
    fn history_orders_hours_fills_gaps_and_adds_duplicate_buckets() {
        let start = DateTime::parse_from_rfc3339("2026-09-25T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let body = json!({"data":{"viewer":{"accounts":[{"series":[
            {"dimensions":{"datetimeHour":"2026-09-25T14:00:00Z"},"sum":{"a":20,"b":2}},
            {"dimensions":{"datetimeHour":"2026-09-25T12:00:00Z"},"sum":{"a":10,"b":1}},
            {"dimensions":{"datetimeHour":"2026-09-25T14:00:00Z"},"sum":{"a":3,"b":4}}
        ]}]}}});
        let history = parse_history(&body, "series", "a b", start).unwrap();
        assert_eq!(&history.first[..4], &[10, 0, 23, 0]);
        assert_eq!(history.second[2], 6);
        assert_eq!(history.first.len(), 24);
        assert!(parse_history(&body, "series", "a b", start + Duration::hours(1)).is_err());
    }
    #[test]
    fn unavailable_is_not_zero() {
        assert!(parse_rows(
            &json!({"data":{"viewer":{"accounts":[]}}}),
            "workersInvocationsAdaptive",
            "requests errors",
            "scriptName"
        )
        .is_err());
        assert!(parse_rows(
            &json!({"errors":[{"message":"denied"}]}),
            "workersInvocationsAdaptive",
            "requests errors",
            "scriptName"
        )
        .is_err());
    }
    #[test]
    fn sorts_resources_and_accepts_empty_dataset() {
        let body = json!({"data":{"viewer":{"accounts":[{"workersInvocationsAdaptive":[{"dimensions":{"scriptName":"a"},"sum":{"requests":1,"errors":0}},{"dimensions":{"scriptName":"b"},"sum":{"requests":9,"errors":2}}],"d1AnalyticsAdaptiveGroups":[]}]}}});
        let rows = parse_rows(
            &body,
            "workersInvocationsAdaptive",
            "requests errors",
            "scriptName",
        )
        .unwrap();
        assert_eq!(rows[0].name, "b");
        assert_eq!(rows[0].second, 2);
        assert!(parse_rows(
            &body,
            "d1AnalyticsAdaptiveGroups",
            "rowsRead rowsWritten",
            "databaseId"
        )
        .unwrap()
        .is_empty());
    }
}
