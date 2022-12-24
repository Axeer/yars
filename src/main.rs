use {
    std::io::prelude::*,
    std::net::TcpListener,
    std::net::TcpStream,
    std::fs,
    std::str,
    std::cmp::Ordering,
    cli_clipboard,
};

const MAX_SIZE: usize = 128 * 1024;
const BASE_ADDRESS: &'static str = "127.0.0.1:8888";
const NAME_PATH_SPLITTER: &'static str = "$";

type TsizeBuf<'a, const N: usize> = &'a [u8; N];

fn main() {
    let listener = TcpListener::bind(BASE_ADDRESS).unwrap();
    set_clipboard();
    for stream in listener.incoming() {
        let stream = stream.unwrap();
        handle_connection(stream)
    }
}

struct File {
    name: String,
    path: String,
}

trait FileT {
    fn get_filename(&self) -> &str;
    fn set_filename(&mut self, new_name: &str) -> &Self;
    fn get_path(&self) -> String;
    fn set_path(&mut self, new_path: String) -> &Self;
}

#[derive(Clone)]
struct Files {
    files: Vec<File>,
}

impl Files {
    fn new(files: Vec<String>) -> Files {
        let files = Files {
            files: vec![]
        };
        //files.init_file();
        files
    }

    fn init_file(&self) -> &Self {
        let mut file = std::fs::File::create("tmp.vfs").unwrap();
        let mut buf = String::new();
        for file in &self.files {
            buf += file.get_filename();
            buf += NAME_PATH_SPLITTER;
            buf += file.get_path().as_str();
            buf += "\n";
        }
        file.write(buf.as_ref()).expect("cant write file");
        self
    }
}

trait FilesT {
    fn add_file(&mut self, file: File) -> &Self;
    fn rem_file(&mut self, file_name: String) -> &Self;
    fn get_file(&self, file_name: String) -> Option<&File>;
}

impl File {
    fn new(name: String, path: String) -> File {
        let path = match path.len().cmp(&0) {
            Ordering::Equal => { String::from(".\\") + name.clone().as_str() }
            Ordering::Greater => { path }
            _ => { String::default() }
        };

        File {
            name: name.clone(),
            path,
        }
    }
}

impl Clone for File {
    fn clone(&self) -> File {
        File::new(
            self.name.clone(),
            self.path.clone(),
        )
    }
}

impl FileT for File {
    fn get_filename(&self) -> &str {
        self.name.as_str().clone()
    }

    fn set_filename(&mut self, new_name: &str) -> &Self {
        self.name = new_name.clone().parse().unwrap();
        self
    }

    fn get_path(&self) -> String {
        self.path.clone()
    }

    fn set_path(&mut self, new_path: String) -> &Self {
        self.path = new_path.clone();
        self
    }
}

impl FilesT for Files {
    fn add_file(&mut self, file: File) -> &Self {
        self.files.push(file);
        self
    }

    fn rem_file(&mut self, file_name: String) -> &Self {
        let mut index: usize = 0;
        for file in self.files.clone() {
            if file.name == file_name {
                self.files.remove(index);
            }
            index += 1;
        }
        self
    }

    fn get_file(&self, file_name: String) -> Option<&File> {
        self.files.iter().find(|file| file.name == file_name)
    }

}

struct Vfs {
    files: Files,
}

trait VfsT {
    fn make_address(file: &File) -> String;
    fn allocate(&mut self, file_name: String) -> String;
    fn path_by_name(&self, name: String) -> Option<String>;
    fn get_file_by_name(&self, name: String) -> Option<File>;
}

impl Vfs {
    fn new() -> Vfs {
        Vfs {
            files: Vfs::read_tmpvfs()
        }
    }

    fn read_tmpvfs() -> Files {
        let files = fs::read_to_string("./tmp.vfs").unwrap();
        let files: Vec<&str> = files.split("\r\n").clone().collect();
        let mut out = Files::new(vec![]);

        for file in files {
            let name_path: Vec<&str> = file.trim().split(NAME_PATH_SPLITTER).collect();
            out.add_file(File::new(
                name_path.get(0).unwrap().to_string(),
                name_path.get(1).unwrap_or(&"").to_string())
            );
        }

        out
    }
}

impl VfsT for Vfs {
    fn make_address(file: &File) -> String {
        format!("{}/{}", BASE_ADDRESS, file.get_filename())
    }

    fn allocate(&mut self, file_name: String) -> String {
        let file = self.files.get_file(file_name).unwrap();
        Vfs::make_address(file)
    }

    fn path_by_name(&self, name: String) -> Option<String> {
        for file in self.files.files.clone().into_iter() {
            if file.name == name {
                return Some(file.path);
            }
        }

        return None;
    }

    fn get_file_by_name(&self, name: String) -> Option<File> {
        for file in self.files.files.clone().into_iter() {
            if file.name == name {
                return Some(file);
            }
        }

        return None;
    }
}

fn make_contents(file_name: &str) -> Vec<u8> {
    let mut contents = fs::read(file_name)
        .unwrap_or(fs::read("tmp.vfs").unwrap());
    if contents.len() > MAX_SIZE {
        contents = "File is bigger than 128 KB".as_bytes().to_vec();
    }

    contents
}

fn make_response(file: &File) -> Vec<u8> {
    let status_line = "HTTP/1.1 200 OK";
    let contents = make_contents(file.get_path().as_str());
    let response = format!(
        "{}\r\nContent-Length: {}\r\n\r\n",
        status_line,
        contents.len(),
    );
    let response: Vec<u8> = [response.as_bytes().to_vec(), contents].concat();

    response
}

fn bytes2string<const N: usize>(bytes: TsizeBuf<N>) -> String {
    str::from_utf8(bytes).unwrap().to_string().clone()
}

fn slice_end(input: String, start: usize) -> String {
    input.get(start..input.find("\0").unwrap()).unwrap().to_string()
}

fn log_buffer<const N: usize>(buffer: TsizeBuf<N>) {
    println!("RAW BUFFER DATA: {:#?}", slice_end(bytes2string(&buffer), 0))
}

fn response_file(headers: &Vec<&str>, stream: &mut TcpStream) {
    let vfs = Vfs::new();
    let mut requested_file: String = String::new();
    match Some(headers.get(1)) {
        None => return,
        Some(_) => requested_file = headers.get(1).unwrap().trim_start_matches('/').to_string()
    }
    let file = vfs.get_file_by_name(requested_file).unwrap_or(File::new("tmp.vfs".to_string(), "".to_string()));

    stream.write(&make_response(&file)).unwrap();
    stream.flush().unwrap();
}

fn set_clipboard() -> bool {
    let string = format!("wget -f http://{}/", BASE_ADDRESS);
    cli_clipboard::set_contents(string.to_owned()).unwrap();
    cli_clipboard::get_contents().unwrap() == string
}

fn handle_connection(mut stream: TcpStream) {
    let mut buffer = [0u8; 8192];
    stream.read(&mut buffer).unwrap();

    #[cfg(debug_assertions)]
    log_buffer(&buffer);

    let headers = bytes2string(&buffer);
    let headers: Vec<&str> = headers.split_whitespace().collect();

    if headers.get(0).unwrap() == &"POST" {
        let mut data: String = bytes2string(&buffer);
        let headers_len = data.find("\r\n\r\n").unwrap();
        data = slice_end(data, headers_len+"\r\n\r\n".len());

        let file_name = headers.get(1).unwrap().to_string();
        fs::File::create(format!("./{}", file_name)).unwrap().write(data.as_bytes()).unwrap();
        #[cfg(debug_assertions)]
        println!("POST DATA: {:#?}", data);
    } else {
        response_file(&headers, &mut stream);
    }
}