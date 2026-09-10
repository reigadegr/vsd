use crate::{
    error::Result,
    playlist::{MediaPlaylist, MediaType},
};
use colored::Colorize;
use log::info;
use requestty::{Question, question::Choice};
use std::io::{self, Write};

#[derive(Clone, Default)]
pub enum SelectType {
    Modern,
    #[default]
    None,
    Raw,
}

pub fn select(
    streams: Vec<MediaPlaylist>,
    selected: &[usize],
    select_type: &SelectType,
) -> Result<Vec<MediaPlaylist>> {
    let selected = match select_type {
        SelectType::Modern => &interact_modern(&streams, selected)?,
        SelectType::Raw => &interact_raw(&streams, selected)?,
        SelectType::None => {
            for (i, stream) in streams.iter().enumerate() {
                info!(
                    "Stream [{}] {}",
                    stream.media_type.to_string().yellow(),
                    if selected.contains(&i) {
                        stream.to_string().cyan()
                    } else {
                        stream.to_string().dimmed()
                    }
                );
            }
            selected
        }
    };

    Ok(streams
        .into_iter()
        .enumerate()
        .filter_map(|(i, s)| if selected.contains(&i) { Some(s) } else { None })
        .collect())
}

#[allow(clippy::type_complexity)]
fn build_choices(
    streams: &[MediaPlaylist],
    selected: &[usize],
) -> (Vec<Choice<(String, bool)>>, Vec<Option<usize>>) {
    let mut choices = Vec::new();
    let mut choice_to_stream = Vec::new();

    for (media_type, header) in [
        (MediaType::Video, "─────── Video Streams ────────"),
        (MediaType::Audio, "─────── Audio Streams ────────"),
        (MediaType::Subtitles, "────── Subtitle Streams ──────"),
        (MediaType::Undefined, "───── Undefined Streams ──────"),
    ] {
        let streams = streams
            .iter()
            .enumerate()
            .filter(|(_, s)| s.media_type == media_type)
            .collect::<Vec<_>>();

        if streams.is_empty() {
            continue;
        }

        choices.push(Choice::Separator(header.to_owned()));
        choice_to_stream.push(None);

        for (i, stream) in streams {
            choices.push(Choice::Choice((stream.to_string(), selected.contains(&i))));
            choice_to_stream.push(Some(i));
        }
    }

    (choices, choice_to_stream)
}

fn interact_modern(streams: &[MediaPlaylist], selected: &[usize]) -> Result<Vec<usize>> {
    let (choices, choice_to_stream) = build_choices(streams, selected);

    let question = Question::multi_select("streams")
        .message("Select streams to download")
        .choices_with_default(choices)
        .transform(|choices, _, backend| {
            let summary = choices
                .iter()
                .map(|x| {
                    x.text
                        .split('|')
                        .map(str::trim)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join(" | ");
            backend.write_styled(&requestty::prompt::style::Stylize::cyan(&summary))
        })
        .build();

    let answer = requestty::prompt_one(question)?;
    let selected = answer
        .as_list_items()
        .unwrap()
        .iter()
        .filter_map(|item| choice_to_stream[item.index])
        .collect();
    Ok(selected)
}

fn interact_raw(streams: &[MediaPlaylist], selected: &[usize]) -> Result<Vec<usize>> {
    let (choices, choice_to_stream) = build_choices(streams, selected);
    let stream_order = choice_to_stream
        .iter()
        .filter_map(|x| *x)
        .collect::<Vec<_>>();

    info!("Select streams to download:");

    let mut choice_num = 1_usize;
    let mut defaults = Vec::new();

    for choice in choices {
        match choice {
            Choice::Separator(header) => info!("{}", header.replace('─', "-").cyan()),
            Choice::Choice((choice, selected)) => {
                if selected {
                    defaults.push(choice_num);
                }
                info!(
                    "{:>2}) [{}] {}",
                    choice_num,
                    if selected { "x".green() } else { " ".normal() },
                    choice
                );
                choice_num += 1;
            }
            _ => (),
        }
    }

    info!("{}", "------------------------------".cyan());
    print!("Press enter to proceed with defaults.\nOr select streams (1, 2, etc.): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    info!("{}", "------------------------------".cyan());

    let user_choices = if input.trim().is_empty() {
        defaults
    } else {
        input
            .trim()
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect()
    };

    let selected = user_choices
        .into_iter()
        .filter_map(|n| stream_order.get(n.checked_sub(1)?).copied())
        .collect::<Vec<_>>();

    for &i in &selected {
        if let Some(stream) = streams.get(i) {
            info!(
                "Stream [{}] {}",
                stream.media_type.to_string().yellow(),
                stream.to_string().cyan()
            );
        }
    }

    info!("{}", "------------------------------".cyan());
    Ok(selected)
}
