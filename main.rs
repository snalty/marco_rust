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
use gio::prelude::*;
use gtk::prelude::*;
use gtk::{ApplicationWindow, Image};
use mime::IMAGE_BMP;
use rocket::State;
use rocket::fs::NamedFile;
use rocket::response::content;
use std::thread;
use gdk_pixbuf::Pixbuf;
use std::sync::{Arc};
use rusqlite::{params, Connection, Result};
use ::serde::{Deserialize, Serialize};
use clap::Arg;
use std::fmt::Display;
use std::fs;

#[tokio::main]
async fn main() {
    let application = gtk::Application::new(
        Some("com.github.gtk-rs.examples.communication_thread"),
        Default::default(),
    );

    application.connect_activate(build_ui);
    application.run();
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
    
    fn update_library(&mut self) {
        let mut query = self.database.prepare("SELECT * FROM images")
        .unwrap();
        
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
    }
}

// TODO: Handle image cropping on the image upload flow
fn load_image(path: &str) -> Pixbuf {
    let start = std::time::SystemTime::now();
    let mut  image_pixbuf = Pixbuf::from_file(&path).unwrap();
    image_pixbuf = image_pixbuf.scale_simple(1024, 768, gdk_pixbuf::InterpType::Bilinear)
    .unwrap();
    return image_pixbuf;
}

fn build_ui(application: &gtk::Application) {
    let matches = clap::App::new("marco")
    .version("alpha")
    .about("Embedded marco photo frame application.")
    .author("@snalty")
    .arg(Arg::with_name("database-path")
    .short('d')
    .long("database")
    .value_name("DATABASE_PATH")
    .help("Set the path to the database")
    .takes_value(true))
    .get_matches();
    
    let database_path = "/usr/local/share/marco/db.sqlite";
    
    let db = Connection::open(database_path).unwrap();
    
    db.execute(
        "create table if not exists images (
            image_id integer primary key,
            image_path text not null unique,
            thumb_path text not null unique,
            date_added integer not null,
            date_created integer not null,
            favourite integer not null
        )",
        params![]
    ).unwrap();
    
    let frame_controller = Arc::new(Mutex::new(photo_frame_setup(db)));
    let window = ApplicationWindow::new(application);
    window.set_default_size(1024, 768);
    window.set_resizable(false);
    println!("gonna try printing the image path");
    let pfl = frame_controller.try_lock().unwrap();
    
    let image = Image::from_pixbuf(
        Some(
            &load_image(
                pfl.library.images[pfl.current_photo as usize].image_path.as_str())
            )
    );
    
    drop(pfl);
    window.add(&image);
    
    
    // Create a channel between communication thread and main event loop:
    let (sender, receiver) = mpsc::channel(1000);
    spawn_local_handler(image, receiver);
    start_communication_thread(sender, frame_controller);
    
    window.show_all();
    }
    
fn photo_frame_setup(db: Connection) -> FrameController {
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

/// Spawn channel receive task on the main event loop.
fn spawn_local_handler(image: gtk::Image, mut receiver: mpsc::Receiver<String>) {
    let main_context = glib::MainContext::default();
    let future = async move {
        while let Some(file) = receiver.next().await {
            let start = std::time::SystemTime::now();
            println!("loading image");
            let new_image = load_image(&file);
            println!("took {}s to load and crop image", start.elapsed().unwrap().as_secs());
            image.set_from_pixbuf(Some(&new_image));
            let elapsed_time = start.elapsed().unwrap().as_secs();
            println!("image loaded in {}s :)", elapsed_time);
        }
    };
    main_context.spawn_local(future);
}

// Spawn separate thread to handle communication.
fn start_communication_thread(sender: mpsc::Sender<String>, frame_controller: Arc<Mutex<FrameController>>) {
    // Note that blocking I/O with threads can be prevented
    // by using asynchronous code, which is often a better
    // choice. For the sake of this example, we showcase the
    // way to use a thread when there is no other option.

    tokio::spawn(async {
        println!("Are we here?");
        rocket::build()
            .manage(Arc::new(Mutex::new(sender)))
            .manage(frame_controller)
            .mount("/", routes![root, get_thumb, get_current])
            .launch().await;
    });
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


// #[post("/api/add", format = "multipart", data = "<data>")]
// pub async fn image_uploader(content_type: &ContentType, data: Data<'_>, frame_controller: &State<Arc<Mutex<FrameController>>>)
// {
//     let mut options = MultipartFormDataOptions::with_multipart_form_data_fields(
//         vec! [
//         MultipartFormDataField::file("image").content_type_by_string(Some(mime::IMAGE_STAR)).unwrap(),
//         MultipartFormDataField::file("thumbnail").content_type_by_string(Some(mime::IMAGE_STAR)).unwrap(),
//         ]
//     );
    
//     let mut form_data = MultipartFormData::parse(content_type, data, options).await.unwrap();
    
//     let image = form_data.files.get("image");
//     let thumb = form_data.files.get("thumbnail");

//     let mut file_name: Option<&String> = None;
//     let mut image_path: Option<&PathBuf> = None;
//     let mut thumb_path: Option<&PathBuf> = None;
    
//     if let Some(file_fields) = image {
//         let file_field = &file_fields[0];
//         file_name = Some(file_field.file_name.as_ref().unwrap());
//         image_path = Some(&file_field.path);
//     }

//     if let Some(file_fields) = thumb {
//         let file_field = &file_fields[0];
//         thumb_path = Some(&file_field.path);
//     }

//     let record = ImageRecord {
//         image_id: 0,
//         image_path: format!("/usr/local/share/marco/images/{}", file_name.unwrap()),
//         thumb_path: format!("/usr/local/share/marco/thumbs/{}", file_name.unwrap()),
//         date_added: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() as i32,
//         date_created: 0,
//         favourite: false
//     };

//     let conn = &frame_controller.lock().await.database;
//     conn.execute("INSERT INTO images 
//     (image_path, thumb_path, date_added, date_created, favourite) 
//     VALUES (?1, ?2, ?3, ?4, ?5)", 
//     params![record.image_path, record.thumb_path, record.date_added, 
//         record.date_created, record.favourite]).unwrap();

//     fs::rename(image_path.unwrap(), record.image_path).unwrap();
//     fs::rename(thumb_path.unwrap(), record.thumb_path).unwrap();

//     // Update library from database

//     &frame_controller.lock()
//         .await
//         .update_library();
// }

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
