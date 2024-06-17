use human_panic::setup_panic;
use indicatif::ProgressBar;
use jomini::JominiDeserialize;
use rand::Rng;
use rayon::prelude::*;
use std::{collections::HashMap, io::BufRead, path::PathBuf, sync::Arc, thread};

struct Province {
    rgba: [u8; 4],
    number: isize,
    is_land: bool,
}

fn count_lines(file: &PathBuf) -> usize {
    std::io::BufReader::new(std::fs::File::open(file).unwrap())
        .lines()
        .count()
}

fn get_provinces(map_directory: &PathBuf) -> Vec<Province> {
    let definition_path = map_directory.join("definition.csv");

    let definition_file = std::fs::File::open(&definition_path).unwrap();
    let mut provinces = Vec::new();

    let pb = ProgressBar::new(count_lines(&definition_path) as u64);
    pb.set_prefix("Loading provinces...");
    pb.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
            .unwrap(),
    );
    pb.set_position(0);

    for line in std::io::BufReader::new(&definition_file).lines() {
        let line = line.unwrap();

        if line.is_empty() {
            continue;
        }

        pb.set_message(line.clone());

        let record = line.split(';').collect::<Vec<_>>();

        let number = record.get(0).unwrap().parse::<isize>().unwrap();

        let rgba = [
            record.get(1).unwrap().parse::<u8>().unwrap(),
            record.get(2).unwrap().parse::<u8>().unwrap(),
            record.get(3).unwrap().parse::<u8>().unwrap(),
            255,
        ];

        let type_ = record.get(4).unwrap();

        provinces.push(Province {
            rgba,
            number,
            is_land: *type_ == "land",
        });

        pb.inc(1);
    }

    pb.finish();

    provinces
}

#[derive(JominiDeserialize)]
struct State {
    pub id: isize,
    pub provinces: Vec<isize>,
}

fn get_states(hoi_directory: PathBuf) -> Vec<State> {
    let states_path = hoi_directory.join("history").join("states");

    let mut states = Vec::new();

    for file in std::fs::read_dir(&states_path).unwrap() {
        let file = file.unwrap();

        let content = std::fs::read(&file.path()).unwrap();
        let file = jomini::TextTape::from_slice(&content).unwrap();
        let reader = file.windows1252_reader();

        let state = reader
            .fields()
            .next()
            .unwrap()
            .2
            .read_object()
            .unwrap()
            .deserialize()
            .unwrap();

        states.push(state);
    }

    states
}

fn get_countries_colors(hoi_directory: PathBuf) -> HashMap<String, [u8; 4]> {
    let tags_path = hoi_directory
        .join("common")
        .join("country_tags")
        .join("00_countries.txt");
    let mut tags_and_path = HashMap::new();

    for line in std::io::BufReader::new(std::fs::File::open(tags_path).unwrap()).lines() {
        let line = line.unwrap();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('#') {
            continue;
        }

        let record = line.split('=').collect::<Vec<_>>();
        let path = record[1].trim_start().trim_end();
        let first = path.find('\"').unwrap();
        let last = path.rfind('\"').unwrap();

        let path = path[first + 1..last].to_string();
        let p = path.split_once('/').unwrap();

        tags_and_path.insert(
            record[0].to_string().trim().to_string(),
            PathBuf::from(p.0).join(p.1),
        );
    }

    let mut countries_colors = HashMap::new();

    for (k, v) in tags_and_path {
        let path = hoi_directory.join("common").join(&v);

        let content = std::fs::read_to_string(&path).unwrap();

        let first = content.find('{').unwrap();
        let last = content.find('}').unwrap();

        let content = content[first + 1..last].to_string();

        let mut country_color = content
            .split(' ')
            .filter(|x| !x.is_empty())
            .map(|x| x.parse::<u8>().unwrap())
            .collect::<Vec<_>>();

        loop {
            let val = countries_colors
                .iter()
                .find(|x| *x.1 == [country_color[0], country_color[1], country_color[2], 255]);

            if val.is_some() {
                country_color[0] = rand::thread_rng().gen_range(0..=255);
                country_color[1] = rand::thread_rng().gen_range(0..=255);
                country_color[2] = rand::thread_rng().gen_range(0..=255);
            } else {
                break;
            }
        }

        countries_colors.insert(
            k,
            [country_color[0], country_color[1], country_color[2], 255],
        );
    }

    countries_colors
}

#[tokio::main]
async fn main() {
    setup_panic!();

    let args = std::env::args().collect::<Vec<_>>();

    let hoi_directory = PathBuf::from(&args[1]);
    let map_directory = hoi_directory.join("map");
    let save_path = PathBuf::from(&args[2]);

    let provinces_path = map_directory.join("provinces.bmp");
    let raw = image::open(provinces_path).unwrap();

    let file_content = std::fs::read(save_path).unwrap();
    let save = hoi4save::Hoi4File::from_slice(&file_content)
        .unwrap()
        .parse()
        .unwrap();

    let countries_colors = get_countries_colors(hoi_directory.clone());

    let reader = save.as_text().unwrap().reader();

    let states = get_states(hoi_directory);

    let mut provinces_and_colors = Vec::new();

    for (k, _, v) in reader.fields() {
        if k.read_str() == "states" {
            for (id, _, val) in v.read_object().unwrap().fields() {
                let state = val.read_object().unwrap();

                let owner = state
                    .fields()
                    .find(|x| x.0.read_str() == "owner")
                    .unwrap()
                    .2
                    .read_str()
                    .unwrap();

                if let Some(color) = countries_colors.get(&owner.to_string()) {
                    let id = id.read_scalar().to_i64().unwrap() as isize;

                    if let Some(state) = states.iter().find(|x| x.id == id) {
                        provinces_and_colors.push((state.provinces.clone(), *color));
                    }
                } else {
                    let id = id.read_scalar().to_i64().unwrap() as isize;

                    if let Some(state) = states.iter().find(|x| x.id == id) {
                        provinces_and_colors.push((state.provinces.clone(), [0, 0, 0, 0]));
                    }
                }
            }
        }
    }

    let mut image = raw.to_rgba8();
    let mut pr = Arc::new(get_provinces(&map_directory));

    let task = thread::spawn(move || {
        let pb = ProgressBar::new(image.pixels().len() as u64);
        pb.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
                .unwrap(),
        );
        pb.set_position(0);

        for (provinces_numbers, province_color) in provinces_and_colors {
            for province_number in provinces_numbers {
                let province = pr
                    .par_iter()
                    .find_first(|x| x.number == province_number)
                    .unwrap();

                image.par_pixels_mut().for_each(|pixel| {
                    if pixel.0 == province.rgba {
                        *pixel = image::Rgba(province_color);

                        pb.inc(1);
                    }
                });
            }
        }
        pb.finish();

        let pb = ProgressBar::new(image.pixels().len() as u64);
        pb.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
                .unwrap(),
        );
        pb.set_position(0);
        pb.set_message("Removing seas...");

        for pr in pr.iter() {
            if !pr.is_land {
                image.par_pixels_mut().for_each(|pixel| {
                    if pixel.0 == pr.rgba {
                        *pixel = image::Rgba([0, 0, 0, 0]);
                        pb.inc(1);
                    }
                });
            }
        }

        pb.finish();

        image.save("output.png").unwrap();
    });

    task.join().unwrap();

    let config = vtracer::Config {
        ..Default::default()
    };

    let input_path = PathBuf::from("output.png");
    let output_path = PathBuf::from("output.svg");

    vtracer::convert_image_to_svg(&input_path, &output_path, config).unwrap();

    let information = serde_json::json!({
        "countries": countries_colors
    });

    std::fs::write("information.json", serde_json::to_string_pretty(&information).unwrap()).unwrap();
}
