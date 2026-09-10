use colored::{ColoredString, Colorize};
use log::{Level, LevelFilter, Metadata, Record};

pub struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            match log::max_level() {
                LevelFilter::Off => (),
                LevelFilter::Error | LevelFilter::Warn | LevelFilter::Info => {
                    if record.level() == Level::Info {
                        println!("{}", record.args());
                    } else {
                        let mut label = label(record.level());
                        label.input = label.input.trim_start().to_owned();
                        println!("{} {}", label, record.args());
                    }
                }
                LevelFilter::Debug | LevelFilter::Trace if record.target().starts_with("vsd") => {
                    let location = match (record.file(), record.line()) {
                        (Some(file), Some(line)) => {
                            format!("{:>30}", format!("{}:{}", file, line)).dimmed()
                        }
                        _ => format!("{:>30}", "unk:unk").dimmed(),
                    };

                    println!("{}{} {}", label(record.level()), location, record.args());
                }
                _ => (),
            }
        }
    }

    fn flush(&self) {}
}

fn label(level: Level) -> ColoredString {
    match level {
        Level::Debug => "[DEBUG]".bold().blue(),
        Level::Error => "[ERROR]".bold().red(),
        Level::Info => " [INFO]".bold().green(),
        Level::Trace => "[TRACE]".bold().purple(),
        Level::Warn => " [WARN]".bold().yellow(),
    }
}
