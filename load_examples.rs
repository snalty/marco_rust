use std::fs;

use rusqlite::{params, Connection, Result};

struct ImageRecord {
    image_id: i32,
    image_path: String,
    thumb_path: String,
    date_added: i32,
    date_created: i32,
    favourite: bool
}

fn main()
{
    let mut conn = Connection::open("/usr/local/share/marco/db.sqlite").unwrap();
    let transaction = conn.transaction().unwrap();
    let files = fs::read_dir("/usr/local/share/marco/images")
        .unwrap()
        .map(|x| x.unwrap());
    let mut records: Vec<ImageRecord> = Vec::new();
    let mut i = 0;
    for file in files {
        let record = ImageRecord {
            image_id: i,
            image_path: String::from(file.path()
                .to_str()
                .unwrap()),
            thumb_path: format!("/usr/local/share/marco/thumbs/{}", String::from(file.file_name()
                .to_str()
                .unwrap())),
            date_added: 0,
            date_created: 0,
            favourite: false
        };
        records.push(record);
        i += 1;
    }
    for record in records {
        transaction.execute("INSERT INTO images 
        (image_id, image_path, thumb_path, date_added, date_created, favourite) 
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)", 
        params![record.image_id, record.image_path, record.thumb_path, record.date_added, 
            record.date_created, record.favourite]).unwrap();
    }
    let _commit = transaction.commit();
}