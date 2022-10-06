//! Example on how to use a communication thread alongside with the GUI thread.
//!
//! Tricks used here:
//! - Use a channel to show data on the GUI.
//! - Run an async function on the GUI event loop.
//! - Use a separate thread to handle incoming data and put it into a channel.
#![feature(proc_macro_hygiene, decl_macro)]
#[macro_use] extern crate rocket;

mod load_examples;

use futures::{channel::mpsc, StreamExt, lock::Mutex};
use mime::IMAGE_BMP;
use rocket::State;
use rocket::fs::{NamedFile, TempFile};
use rocket::response::content;
use rocket::shield::Frame;
use std::path::PathBuf;
use std::thread;
use std::time::SystemTime;
use std::sync::{Arc};
use rusqlite::{params, Result};
use tokio_rusqlite::Connection;
use ::serde::{Deserialize, Serialize};
use clap::Arg;
use std::fmt::Display;
use std::fs;
use rocket::form::Form;
use iced::{Sandbox, Settings, Element, Text};

#[tokio::main]
async fn main() {
    // Get db connection
    let db = Connection::open("/usr/local/share/marco/db.sqlite").await.unwrap();
        
    // Instantiate frame controller
    let frame_controller = Arc::new(Mutex::new(FrameController::new(db)));

    // Launch rocket
    tokio::spawn(async {
        println!("Are we here?");
        rocket::build()
            .manage(frame_controller)
            .mount("/", routes![root, get_thumb, get_current])
            .launch().await;
    });

    PhotoFrame::run(Settings::default());
}

#[derive(Default)]
struct PhotoFrame;

impl Sandbox for PhotoFrame {
    type Message = ();

    fn new() -> PhotoFrame {
        PhotoFrame
    }

    fn title(&self) -> String {
        String::from("A cool application")
    }

    fn update(&mut self, _message: Self::Message) {
        // This application has no interactions
    }

    fn view(&mut self) -> Element<Self::Message> {
        Text::new("hello").size(50).into()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ImageRecord {
    image_id: i32,
    image_path: String,
    thumb_path: String,
    date_added: i32,
    date_created: i32,
    favourite: bool
}

#[derive(Serialize, Deserialize)]
pub struct ImageLibrary {
    images: Vec<ImageRecord>,
}

pub struct FrameController {
    library: Box<ImageLibrary>,
    current_photo: u32,
    database: Connection
}

impl FrameController {
    fn next(&mut self) {
        println!("{}", self.current_photo);
        if self.current_photo == 0 {
            self.current_photo += 1;
        }
        else if self.current_photo + 1 == self.library.images.len() as u32 {
            self.current_photo = 0;
        }
        else {
            self.current_photo += 1;
        }
    }

    fn new(db: Connection) -> FrameController {
        let library = ImageLibrary {
            images: Vec::new()
        };
        let mut frame_controller = FrameController{
            library: Box::new(library),
            current_photo: 0,
            database: db
        };
        frame_controller.update_library();
        return frame_controller
    }
   
    async fn update_library(&'static mut self) {
        self.database
            .call(move |db| {
                let mut query = db.prepare("SELECT * FROM images").unwrap();
                let image_rows = query.query_map(params![], |row| {
                    Ok(ImageRecord {
                        image_id: row.get("image_id").unwrap(),
                        image_path: row.get("image_path").unwrap(),
                        thumb_path: row.get("thumb_path").unwrap(),
                        date_added: row.get("date_added").unwrap(),
                        date_created: row.get("date_created").unwrap(),
                        favourite: row.get("favourite").unwrap(),
                    })
                }).unwrap();
                self.library.images = image_rows.map(|x| x.unwrap()).collect();
            }).await;
    }
}

#[get("/api/next")]
async fn root(sender: &State<Arc<Mutex<mpsc::Sender<String>>>>, frame_controller: &State<Arc<Mutex<FrameController>>>) -> content::RawJson<String> {
    let mut pfl = frame_controller.lock().await;
    pfl.next();
    let image = &pfl.library.images[pfl.current_photo as usize];
    sender.inner()
        .lock()
        .await
        .try_send(image.image_path.to_string()).unwrap();
    
    return content::RawJson(
        serde_json::to_string(image).unwrap()
    );
}


#[derive(FromForm)]
struct ImageUpload<'r> {
    image: TempFile<'r>,
    thumbnail: Option<TempFile<'r>>,
}

#[post("/api/add", data = "<image_upload>")]
pub async fn image_uploader(image_upload: Form<ImageUpload<'_>>, frame_controller: &State<Arc<Mutex<FrameController>>>) {
        
    let image = &image_upload.image;
    let thumb = &image_upload.thumbnail;

    let file_name = image.name();
    let image_path = format!("/usr/local/share/marco/images/{}", file_name.unwrap());
    let thumb_path = format!("/usr/local/share/marco/thumbs/{}", file_name.unwrap());

    let record = ImageRecord {
        image_id: 0,
        image_path: image_path.clone(),
        thumb_path: thumb_path.clone(),
        date_added: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() as i32,
        date_created: 0,
        favourite: false
    };

    // let conn = &frame_controller.lock().await.database;
    // conn.execute("INSERT INTO images 
    // (image_path, thumb_path, date_added, date_created, favourite) 
    // VALUES (?1, ?2, ?3, ?4, ?5)", 
    // params![record.image_path, record.thumb_path, record.date_added, 
    //     record.date_created, record.favourite]).unwrap();

    fs::rename(image_path, record.image_path).unwrap();
    fs::rename(thumb_path, record.thumb_path).unwrap();

    // Update library from database

    &frame_controller.lock()
        .await
        .update_library();
}

#[get("/api/library")]
pub async fn get_library(frame_controller: &State<Arc<Mutex<FrameController>>>) -> content::RawJson<String>
{
    let images = &frame_controller.lock().await.library.images;
    let library = ImageLibrary {
        images: images.to_vec()
    };
    let json = serde_json::to_string(&library).unwrap();
    return content::RawJson(json)
}

#[get("/api/thumb?<image_id>")]
pub async fn get_thumb(image_id: i32, frame_controller: &State<Arc<Mutex<FrameController>>>) -> Option<NamedFile>
{
    let thumb_path = &frame_controller
        .lock()
        .await
        .library
        .images[image_id as usize]
        .thumb_path;
    NamedFile::open(thumb_path).await.ok()
}

#[get("/api/current")]
pub async fn get_current(frame_controller: &State<Arc<Mutex<FrameController>>>) -> content::RawJson<String> 
{
    let controller = frame_controller
        .lock()
        .await;

    let current_id = controller.current_photo;  

    let current_photo = &controller
        .library
        .images[current_id as usize];

    content::RawJson(
        serde_json::to_string(current_photo).unwrap()
    )
}
