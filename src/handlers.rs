use actix_web::{HttpResponse, Responder, web};
use dtqueue::utils::sanitize_queue_name;
use dtqueue::{AppDb, QueueItem};
use log::{error, info, warn};
use rusqlite::params;

pub fn init_routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/{queue}", web::put().to(put_item))
        .route("/{queue}", web::get().to(get_item))
        .route("/{queue}", web::delete().to(delete_item));
}

async fn put_item(db: web::Data<AppDb>, path: web::Path<String>, body: String) -> impl Responder {
    let queue = path.into_inner();
    if !db.queues.contains(&queue) {
        warn!("Invalid queue name attempted: {queue}");
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <Error>
                <Code>InvalidQueueName</Code>
                <Message>Invalid queue name attempted: {queue}</Message>
            </Error>"#
        );
        return HttpResponse::Forbidden()
            .content_type("text/xml")
            .insert_header(("Content-Length", body.len().to_string()))
            .body(body);
    }

    // parse item from the body
    let item = match QueueItem::from_json_string(&body) {
        Ok(body) => body,
        Err(e) => {
            warn!("Failed to parse request body: {e}");
            let body = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
                <Error>
                    <Code>InvalidRequest</Code>
                    <Message>Failed to parse request body due to: {e}
                    Request body:
{body}
                    </Message>
                </Error>"#
            );
            return HttpResponse::BadRequest()
                .content_type("text/xml")
                .insert_header(("Content-Length", body.len().to_string()))
                .body(body);
        }
    };

    let conn = match db.conn.lock() {
        Ok(conn) => conn,
        Err(e) => {
            error!("Failed to lock database: {e}");
            let body = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
                <Error>
                    <Code>InternalError</Code>
                    <Message>{e}</Message>
                </Error>"#
            );
            return HttpResponse::InternalServerError()
                .content_type("text/xml")
                .insert_header(("Content-Length", body.len().to_string()))
                .body(body);
        }
    };

    let table = sanitize_queue_name(&queue).unwrap();

    // check if the item already exists
    let mut stmt = conn
        .prepare(&format!(
            "SELECT COUNT(*) FROM {table} WHERE datetime = ?1 AND datetime_secondary IS ?2"
        ))
        .expect("invalid SQL statement");
    let count: i64 = stmt
        .query_row(
            params![
                item.datetime.to_rfc3339(),
                item.datetime_secondary.map(|d| d.to_rfc3339())
            ],
            |row| row.get(0),
        )
        .expect("Failed to query item existence");
    if count > 0 {
        warn!("append to queue {queue} failed, duplicated items: {item:?}");
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <Error>
                <Code>ItemAlreadyExists</Code>
                <Message>duplicated item {item:?} in queue {queue}</Message>
            </Error>"#
        );
        return HttpResponse::Conflict()
            .content_type("text/xml")
            .insert_header(("Content-Length", body.len().to_string()))
            .body(body);
    }

    // insert the item
    let mut stmt = conn
        .prepare(&format!(
            "INSERT INTO {table} (datetime, datetime_secondary, message) VALUES (?1, ?2, ?3)"
        ))
        .expect("invalid SQL statement");
    match stmt.execute(params![
        item.datetime.to_rfc3339(),
        item.datetime_secondary.map(|d| d.to_rfc3339()),
        item.message.clone().unwrap_or_default()
    ]) {
        Ok(_) => {
            info!("append to queue {queue} successful, the item is {item:?}");
            HttpResponse::Ok().body("")
        }
        Err(e) => {
            error!("Failed to append {item:?} to '{queue}': {e}");
            let body = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
                <Error>
                    <Code>InternalError</Code>
                    <Message>{e}</Message>
                </Error>"#
            );
            HttpResponse::InternalServerError()
                .content_type("text/xml")
                .insert_header(("Content-Length", body.len().to_string()))
                .body(body)
        }
    }
}

async fn get_item(db: web::Data<AppDb>, path: web::Path<String>) -> impl Responder {
    let queue = path.into_inner();
    if !db.queues.contains(&queue) {
        warn!("Invalid queue name attempted: {queue}");
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <Error>
                <Code>InvalidQueueName</Code>
                <Message>Invalid queue name attempted: {queue}</Message>
            </Error>"#
        );
        return HttpResponse::BadRequest()
            .content_type("text/xml")
            .insert_header(("Content-Length", body.len().to_string()))
            .body(body);
    }

    let conn = match db.conn.lock() {
        Ok(conn) => conn,
        Err(e) => {
            error!("Failed to lock database: {e}");
            let body = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
                <Error>
                    <Code>InternalError</Code>
                    <Message>{e}</Message>
                </Error>"#
            );
            return HttpResponse::InternalServerError()
                .content_type("text/xml")
                .insert_header(("Content-Length", body.len().to_string()))
                .body(body);
        }
    };

    let table = sanitize_queue_name(&queue).unwrap();

    // retrieve the item, ordered by datetime and datetime_secondary
    let mut stmt = conn
        .prepare(&format!(
            "SELECT datetime, datetime_secondary, message FROM {table} WHERE valid = 1 ORDER BY datetime ASC, datetime_secondary ASC LIMIT 1"
        ))
        .expect("invalid SQL statement");
    let item: Option<QueueItem> = stmt
        .query_row(params![], |row| {
            let datetime: String = row.get(0)?;
            let datetime_secondary: Option<String> = row.get(1)?;
            let message: Option<String> = row.get(2)?;
            Ok(QueueItem {
                datetime: chrono::DateTime::parse_from_rfc3339(&datetime)
                    .unwrap()
                    .into(),
                datetime_secondary: datetime_secondary
                    .map(|d| chrono::DateTime::parse_from_rfc3339(&d).unwrap().into()),
                message,
            })
        })
        .ok();

    match item {
        Some(item) => {
            let body = item.to_json_string().unwrap();
            info!("retrieve from queue {queue}, got {item:?}");
            HttpResponse::Ok()
                .content_type("application/json")
                .insert_header(("Content-Length", body.len().to_string()))
                .body(body)
        }
        None => {
            info!("retrieve from queue {queue}, the queue is empty");
            HttpResponse::NoContent().finish()
        }
    }
}

async fn delete_item(db: web::Data<AppDb>, path: web::Path<String>) -> impl Responder {
    let queue = path.into_inner();
    if !db.queues.contains(&queue) {
        warn!("Invalid queue name attempted: {queue}");
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <Error>
                <Code>InvalidQueueName</Code>
                <Message>Invalid queue name attempted: {queue}</Message>
            </Error>"#
        );
        return HttpResponse::BadRequest()
            .content_type("text/xml")
            .insert_header(("Content-Length", body.len().to_string()))
            .body(body);
    }

    let conn = match db.conn.lock() {
        Ok(conn) => conn,
        Err(e) => {
            error!("Failed to lock database: {e}");
            let body = format!(
                r#"<?xml version=\"1.0\" encoding=\"UTF-8\"?>
                <Error>
                    <Code>InternalError</Code>
                    <Message>{e}</Message>
                </Error>"#
            );
            return HttpResponse::InternalServerError()
                .content_type("text/xml")
                .insert_header(("Content-Length", body.len().to_string()))
                .body(body);
        }
    };

    let table = sanitize_queue_name(&queue).unwrap();

    // Find the first valid item
    let mut stmt = conn
        .prepare(&format!(
            "SELECT datetime, datetime_secondary, message FROM {table} WHERE valid = 1 ORDER BY datetime ASC, datetime_secondary ASC LIMIT 1"
        ))
        .expect("invalid SQL statement");
    let result: Option<(String, Option<String>, Option<String>)> = stmt
        .query_row(params![], |row| {
            let datetime: String = row.get(0)?;
            let datetime_secondary: Option<String> = row.get(1)?;
            let message: Option<String> = row.get(2)?;
            Ok((datetime, datetime_secondary, message))
        })
        .ok();

    if let Some((datetime, datetime_secondary, message)) = result {
        let item = QueueItem {
            datetime: chrono::DateTime::parse_from_rfc3339(&datetime)
                .unwrap()
                .into(),
            datetime_secondary: datetime_secondary
                .clone()
                .map(|d| chrono::DateTime::parse_from_rfc3339(&d).unwrap().into()),
            message,
        };
        // Set valid to 0 for this item
        let mut update_stmt = conn
            .prepare(&format!(
                "UPDATE {table} SET valid = 0 WHERE (datetime = ?1) AND (datetime_secondary IS ?2);"
            ))
            .expect("invalid SQL statement");
        let updated = match update_stmt.execute(params![datetime, datetime_secondary]) {
            Ok(rows) => rows,
            Err(e) => {
                error!("Failed to update item in queue {queue}: {e}");
                0
            }
        };
        let body = item.to_json_string().unwrap();
        if updated > 0 {
            info!("pop from queue {queue}, got {item:?}");
            HttpResponse::Ok()
                .content_type("application/json")
                .insert_header(("Content-Length", body.len().to_string()))
                .body(body)
        } else {
            info!("pop from queue {queue}, got {item:?}, but failed to mark as invalid");
            HttpResponse::InternalServerError()
                .content_type("application/json")
                .insert_header(("Content-Length", body.len().to_string()))
                .body(body)
        }
    } else {
        // No valid item found
        info!("pop from queue {queue}, the queue is empty");
        HttpResponse::NoContent().finish()
    }
}
