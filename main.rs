#![feature(let_chains)]

use std::{
    cmp::max,
    io::{prelude::*, BufReader},
    net::{TcpListener, TcpStream},
    sync::LazyLock,
    thread,
};

use num_bigint::BigUint;
use regex::Regex;
use sha1::{Sha1, Digest};

static PLAY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new("POST /api/v1/play\\?prefix=([0-9a-f]*)&size=([0-9]+)&offset=([0-9]+)&start=([0-9a-f]+)&end=([0-9a-f]+) HTTP/1\\.1").unwrap());
static HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new("(.+?): *([^\r\n]+)").unwrap());

fn main() {
    let listener = TcpListener::bind("0.0.0.0:8000").unwrap();

    for stream in listener.incoming() {
        let stream = stream.unwrap();
        thread::spawn(move || {
            handle_connection(stream);
        });
    }
}

fn handle_connection(mut stream: TcpStream) {
    let mut buf_reader = BufReader::new(&mut stream);
    let mut request_line = "".to_string();
    buf_reader.read_line(&mut request_line).unwrap();
    request_line.truncate(request_line.len()-2);

    let mut body_length = 0usize;
    while let mut line = "".to_string() && buf_reader.read_line(&mut line).is_ok() && line != "\r\n" {
        let caps = HEADER_RE.captures(&line).unwrap();
        let name = caps.get(1).unwrap().as_str().to_lowercase();
        let value = caps.get(2).unwrap().as_str();
        match name.as_str() {
            "content-length" => {
                body_length = value.parse::<usize>().unwrap();
            },
            _ => {},
        }
    };

    let (status_line, output) = match PLAY_RE.captures(&request_line) {
        Some(caps) => {
            let prefix_raw = caps.get(1).unwrap();
            let prefix_size = prefix_raw.len();
            let prefix_unpadded = BigUint::parse_bytes(&vec![&prefix_raw.as_str().as_bytes(), &b"0"[..prefix_size % 2]].concat(), 16).unwrap_or_default().to_bytes_be();
            let mut prefix = Vec::new();
            prefix.resize((prefix_size + 1)/2, 0u8);
            prefix.copy_from_slice(&prefix_unpadded[..(prefix_size+1)/2]);
            let size = caps.get(2).unwrap().as_str().parse::<u64>().unwrap();
            let offset = caps.get(3).unwrap().as_str().parse::<u64>().unwrap();
            let mut i = BigUint::parse_bytes(caps.get(4).unwrap().as_str().as_bytes(), 16).unwrap();
            let end = BigUint::parse_bytes(caps.get(5).unwrap().as_str().as_bytes(), 16).unwrap();
            let odd = prefix_size % 2;

            let mut body = vec![0u8; max(body_length, (offset+size) as usize)];
            const SPACES: [u8; 2]  = [32u8, 9u8];
            buf_reader.read_exact(&mut body[..body_length]).unwrap();

            let mut found = 0;
            while i < end {
                for j in 0..size {
                    body[(offset+j) as usize] = SPACES[i.bit(j) as usize];
                }
                let mut hasher = Sha1::new();
                hasher.update(&body);
                let result = hasher.finalize();
                if result[..prefix.len()-odd] == prefix[..prefix.len()-odd] && (odd == 0 || result[prefix.len()-1] & 0xf0 == prefix[prefix.len()-1]) {
                    if found == 0 {
                        stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap();
                    }
                    let chunk_body = format!("{}\n", i.to_str_radix(16));
                    let chunk = format!("{:X}\r\n{chunk_body}\r\n", chunk_body.len());
                    stream.write_all(chunk.as_bytes()).unwrap();
                    found += 1;
                }
                i += 1u8;
            }

            if found > 0 {
                println!("{request_line} 200 OK {found}");
                stream.write_all(b"0\r\n\r\n").unwrap();
                return
            } else {
                ("HTTP/1.1 404 Not Found", "404\n")
            }
        },
        _ => ("HTTP/1.1 400 Bad Request", "400\n"),
    };

    let length = output.len();
    let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{output}");
    let status_code = &status_line[9..];
    println!("{request_line} {status_code}");

    stream.write_all(response.as_bytes()).unwrap();
}
