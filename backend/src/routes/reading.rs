//! Yearly reading stats and goals, built on `book_completions` (one row per
//! finish; see migration 0012 for why that isn't derived from the feed log).
//!
//! Everything here is "this user's non-backdated completions in a period, read
//! in a time zone" — the same shape a friends' challenge or a "read N from this
//! list" target would use later, just with more participants or a book filter.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const MIN_YEAR: i32 = 1900;
const MAX_YEAR: i32 = 2200;
const MAX_TARGET: i32 = 10_000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/reading-stats", get(reading_stats))
        .route(
            "/me/reading-goals/{year}",
            put(set_goal).delete(delete_goal),
        )
}

fn check_year(year: i32) -> ApiResult<i32> {
    if (MIN_YEAR..=MAX_YEAR).contains(&year) {
        Ok(year)
    } else {
        Err(ApiError::BadRequest(format!(
            "year must be between {MIN_YEAR} and {MAX_YEAR}"
        )))
    }
}

/// An IANA zone name the database knows ("America/Los_Angeles", "UTC"). Year
/// boundaries depend on it: a book finished at 6pm on Dec 31 in Seattle is
/// already Jan 1 in UTC.
async fn check_time_zone(pool: &PgPool, tz: &str) -> ApiResult<()> {
    let known = sqlx::query_scalar::<_, bool>(
        "select exists(select 1 from pg_timezone_names where name = $1)",
    )
    .bind(tz)
    .fetch_one(pool)
    .await?;
    if known {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!("unknown time zone '{tz}'")))
    }
}

#[derive(Debug, Deserialize)]
struct StatsParams {
    /// Calendar year to report, default: the current year (UTC).
    year: Option<i32>,
    /// IANA zone the year is read in, default `UTC`. The app sends the device's.
    tz: Option<String>,
}

#[derive(Debug, Serialize)]
struct Goal {
    year: i32,
    target_count: i32,
    time_zone: String,
}

#[derive(Debug, Serialize)]
struct ReadingStats {
    year: i32,
    time_zone: String,
    /// Books finished in the year, rereads counted each time; backdated (read
    /// before using the app) entries excluded.
    completed: i64,
    /// Same, per calendar month, January first.
    by_month: [i64; 12],
    goal: Option<Goal>,
}

async fn reading_stats(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Query(params): Query<StatsParams>,
) -> ApiResult<Json<ReadingStats>> {
    let year = match params.year {
        Some(y) => check_year(y)?,
        None => {
            sqlx::query_scalar::<_, i32>("select extract(year from now() at time zone 'UTC')::int")
                .fetch_one(&pool)
                .await?
        }
    };
    let time_zone = params.tz.unwrap_or_else(|| "UTC".into());
    check_time_zone(&pool, &time_zone).await?;

    // Month buckets are taken in the requested zone, and the year's bounds are
    // instants computed in it, so the index on (user_id, completed_at) is used.
    let rows = sqlx::query_as::<_, (i32, i64)>(
        "select extract(month from completed_at at time zone $2)::int, count(*) \
         from book_completions \
         where user_id = $1 and not backdated \
           and completed_at >= make_timestamptz($3, 1, 1, 0, 0, 0, $2) \
           and completed_at <  make_timestamptz($3 + 1, 1, 1, 0, 0, 0, $2) \
         group by 1",
    )
    .bind(me.id)
    .bind(&time_zone)
    .bind(year)
    .fetch_all(&pool)
    .await?;

    let mut by_month = [0i64; 12];
    for (month, count) in rows {
        by_month[(month - 1) as usize] = count;
    }

    let goal = sqlx::query_as::<_, (i32, String)>(
        "select target_count, time_zone from reading_goals \
         where user_id = $1 and starts_on = make_date($2, 1, 1) and ends_on = make_date($2, 12, 31)",
    )
    .bind(me.id)
    .bind(year)
    .fetch_optional(&pool)
    .await?
    .map(|(target_count, time_zone)| Goal {
        year,
        target_count,
        time_zone,
    });

    Ok(Json(ReadingStats {
        year,
        time_zone,
        completed: by_month.iter().sum(),
        by_month,
        goal,
    }))
}

#[derive(Debug, Deserialize)]
struct SetGoal {
    target_count: i32,
    /// Zone the year is read in; default `UTC`.
    time_zone: Option<String>,
}

/// Create or change the caller's goal for a calendar year.
async fn set_goal(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(year): Path<i32>,
    Json(input): Json<SetGoal>,
) -> ApiResult<Json<Goal>> {
    let year = check_year(year)?;
    if !(1..=MAX_TARGET).contains(&input.target_count) {
        return Err(ApiError::BadRequest(format!(
            "target_count must be between 1 and {MAX_TARGET}"
        )));
    }
    let time_zone = input.time_zone.unwrap_or_else(|| "UTC".into());
    check_time_zone(&pool, &time_zone).await?;

    sqlx::query(
        "insert into reading_goals (user_id, starts_on, ends_on, time_zone, target_count) \
         values ($1, make_date($2, 1, 1), make_date($2, 12, 31), $3, $4) \
         on conflict (user_id, starts_on, ends_on) do update \
             set target_count = excluded.target_count, \
                 time_zone = excluded.time_zone, \
                 updated_at = now()",
    )
    .bind(me.id)
    .bind(year)
    .bind(&time_zone)
    .bind(input.target_count)
    .execute(&pool)
    .await?;

    Ok(Json(Goal {
        year,
        target_count: input.target_count,
        time_zone,
    }))
}

async fn delete_goal(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(year): Path<i32>,
) -> ApiResult<StatusCode> {
    let year = check_year(year)?;
    sqlx::query(
        "delete from reading_goals \
         where user_id = $1 and starts_on = make_date($2, 1, 1) and ends_on = make_date($2, 12, 31)",
    )
    .bind(me.id)
    .bind(year)
    .execute(&pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
