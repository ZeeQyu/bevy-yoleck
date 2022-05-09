use std::path::PathBuf;
use std::{fs, io};

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use bevy::utils::HashMap;
use bevy_egui::egui;

use crate::level_index::YoleckLevelFileEntry;
use crate::{
    YoleckEditorState, YoleckEntryHeader, YoleckManaged, YoleckRawEntry, YoleckTypeHandlers,
};

const EXTENSION: &str = ".yol";
const EXTENSION_WITHOUT_DOT: &str = "yol";

pub struct YoleckEditorLevelsDirectoryPath(pub PathBuf);

pub fn level_files_manager_section(world: &mut World) -> impl FnMut(&mut World, &mut egui::Ui) {
    let mut system_state = SystemState::<(
        Commands,
        ResMut<YoleckEditorLevelsDirectoryPath>,
        Res<YoleckTypeHandlers>,
        Query<(Entity, &YoleckManaged)>,
        ResMut<State<YoleckEditorState>>,
    )>::new(world);

    let mut should_list_files = true;
    let mut loaded_files_index: io::Result<Vec<YoleckLevelFileEntry>> = Ok(vec![]);

    #[derive(Debug)]
    enum SelectedLevelFile {
        Unsaved(String),
        Existing(String),
    }

    let mut selected_level_file = SelectedLevelFile::Unsaved(String::new());

    let mut level_being_playtested: Option<Vec<YoleckRawEntry>> = None;

    move |world, ui: &mut egui::Ui| {
        let (
            mut commands,
            mut levels_directory,
            yoleck_type_handlers,
            yoleck_managed_query,
            mut editor_state,
        ) = system_state.get_mut(world);

        let gen_raw_entries = || {
            yoleck_managed_query
                .iter()
                .map(|(_entity, yoleck_managed)| {
                    let handler = yoleck_type_handlers
                        .type_handlers
                        .get(&yoleck_managed.type_name)
                        .unwrap();
                    YoleckRawEntry {
                        header: YoleckEntryHeader {
                            type_name: yoleck_managed.type_name.clone(),
                            name: yoleck_managed.name.clone(),
                        },
                        data: handler.make_raw(&yoleck_managed.data),
                    }
                })
                .collect::<Vec<_>>()
        };

        let clear_level = |commands: &mut Commands| {
            for (entity, _) in yoleck_managed_query.iter() {
                commands.entity(entity).despawn_recursive();
            }
        };

        ui.horizontal(|ui| {
            if let Some(entries) = &level_being_playtested {
                let finish_playtest_response = ui.button("Finish Playtest");
                if ui.button("Restart Playtest").clicked() {
                    clear_level(&mut commands);
                    for entry in entries.iter() {
                        commands.spawn().insert(entry.clone());
                    }
                }
                if finish_playtest_response.clicked() {
                    clear_level(&mut commands);
                    editor_state.set(YoleckEditorState::EditorActive).unwrap();
                    for entry in entries.iter() {
                        commands.spawn().insert(entry.clone());
                    }
                    level_being_playtested = None;
                }
            } else {
                #[allow(clippy::collapsible_else_if)]
                if ui.button("Playtest").clicked() {
                    let entries = gen_raw_entries();
                    clear_level(&mut commands);
                    editor_state.set(YoleckEditorState::GameActive).unwrap();
                    for entry in entries.iter() {
                        commands.spawn().insert(entry.clone());
                    }
                    level_being_playtested = Some(entries);
                }
            }
        });

        if matches!(editor_state.current(), YoleckEditorState::EditorActive) {
            egui::CollapsingHeader::new("Files")
                .default_open(true)
                .show(ui, |ui| {
                    let mut path_str = levels_directory.0.to_string_lossy().to_string();
                    ui.horizontal(|ui| {
                        ui.label("Levels Directory:");
                        if ui.text_edit_singleline(&mut path_str).lost_focus() {
                            should_list_files = true;
                        }
                    });
                    levels_directory.0 = path_str.into();

                    let mk_files_index = || levels_directory.0.join("index.yoli");

                    let save_index = |loaded_files_index: &[YoleckLevelFileEntry]| {
                        let index_file = mk_files_index();
                        match fs::File::create(&index_file) {
                            Ok(fd) => {
                                serde_json::to_writer(fd, &loaded_files_index).unwrap();
                            }
                            Err(err) => {
                                warn!("Cannot open {:?} - {}", index_file, err);
                            }
                        }
                    };

                    let save_existing = |filename: &str| -> io::Result<()> {
                        let file_path = levels_directory.0.join(filename);
                        info!("Saving current level to {:?}", file_path);
                        let fd = fs::OpenOptions::new()
                            .write(true)
                            .create(false)
                            .truncate(true)
                            .open(file_path)?;
                        serde_json::to_writer(fd, &gen_raw_entries())?;
                        Ok(())
                    };

                    if should_list_files {
                        should_list_files = false;
                        loaded_files_index = fs::read_dir(&levels_directory.0).and_then(|files| {
                            let index_file = mk_files_index();
                            let mut files_index: Vec<YoleckLevelFileEntry> =
                                match fs::File::open(&index_file) {
                                    Ok(fd) => serde_json::from_reader(fd)?,
                                    Err(err) => {
                                        warn!("Cannot open {:?} - {}", index_file, err);
                                        Vec::new()
                                    }
                                };
                            let mut existing_files: HashMap<String, usize> = files_index
                                .iter()
                                .enumerate()
                                .map(|(index, file)| (file.filename.clone(), index))
                                .collect();
                            for file in files {
                                let file = file?;
                                if file.path().extension()
                                    != Some(std::ffi::OsStr::new(EXTENSION_WITHOUT_DOT))
                                {
                                    continue;
                                }
                                let filename = file.file_name().to_string_lossy().into();
                                if existing_files.remove(&filename).is_none() {
                                    files_index.push(YoleckLevelFileEntry { filename });
                                }
                            }
                            // TODO: deal with removed files (leftovers in existing_files)
                            save_index(&files_index);
                            Ok(files_index)
                        });
                    }
                    match &mut loaded_files_index {
                        Ok(files) => {
                            let mut swap_with_previous = None;
                            egui::ScrollArea::vertical()
                                .max_height(30.0)
                                .show(ui, |ui| {
                                    for (index, file) in files.iter().enumerate() {
                                        let is_selected =
                                            if let SelectedLevelFile::Existing(selected_name) =
                                                &selected_level_file
                                            {
                                                *selected_name == file.filename
                                            } else {
                                                false
                                            };
                                        ui.horizontal(|ui| {
                                            if ui.button("^").clicked() {
                                                swap_with_previous = Some(index);
                                            }
                                            if ui.button("v").clicked() {
                                                swap_with_previous = Some(index + 1);
                                            }
                                            if ui
                                                .selectable_label(is_selected, &file.filename)
                                                .clicked()
                                            {
                                                if is_selected {
                                                    save_existing(&file.filename).unwrap();
                                                } else {
                                                    match &selected_level_file {
                                                        SelectedLevelFile::Unsaved(_) => {
                                                            if yoleck_managed_query.is_empty() {
                                                                selected_level_file =
                                                                    SelectedLevelFile::Existing(
                                                                        file.filename.clone(),
                                                                    );
                                                            } else {
                                                                warn!("You have some unsaved file");
                                                                return;
                                                            }
                                                        }
                                                        SelectedLevelFile::Existing(
                                                            current_file,
                                                        ) => {
                                                            save_existing(current_file).unwrap();
                                                            clear_level(&mut commands);
                                                            selected_level_file =
                                                                SelectedLevelFile::Existing(
                                                                    file.filename.clone(),
                                                                );
                                                        }
                                                    }
                                                    let fd = fs::File::open(
                                                        levels_directory.0.join(&file.filename),
                                                    )
                                                    .unwrap();
                                                    let data: Vec<YoleckRawEntry> =
                                                        serde_json::from_reader(fd).unwrap();
                                                    for entry in data.into_iter() {
                                                        commands.spawn().insert(entry);
                                                    }
                                                }
                                            }
                                        });
                                    }
                                });
                            if let Some(swap_with_previous) = swap_with_previous {
                                files.swap(swap_with_previous, swap_with_previous - 1);
                                save_index(files);
                            }
                            ui.horizontal(|ui| {
                                #[allow(clippy::collapsible_else_if)]
                                match &mut selected_level_file {
                                    SelectedLevelFile::Unsaved(file_name) => {
                                        ui.text_edit_singleline(file_name);
                                        let button = ui.add_enabled(
                                            !file_name.is_empty(),
                                            egui::Button::new("Create"),
                                        );
                                        if button.clicked() {
                                            if !file_name.ends_with(EXTENSION) {
                                                file_name.push_str(EXTENSION);
                                            }
                                            let mut file_path = levels_directory.0.clone();
                                            file_path.push(&file_name);
                                            match fs::OpenOptions::new()
                                                .write(true)
                                                .create_new(true)
                                                .open(&file_path)
                                            {
                                                Ok(fd) => {
                                                    info!(
                                                        "Saving current new level to {:?}",
                                                        file_path
                                                    );
                                                    serde_json::to_writer(fd, &gen_raw_entries())
                                                        .unwrap();
                                                    selected_level_file =
                                                        SelectedLevelFile::Existing(
                                                            file_name.to_owned(),
                                                        );
                                                    should_list_files = true;
                                                }
                                                Err(err) => {
                                                    warn!("Cannot open {:?} - {}", file_path, err);
                                                }
                                            }
                                        }
                                    }
                                    SelectedLevelFile::Existing(current_file) => {
                                        if ui.button("New Level").clicked() {
                                            save_existing(current_file).unwrap();
                                            clear_level(&mut commands);
                                            selected_level_file =
                                                SelectedLevelFile::Unsaved(String::new());
                                        }
                                    }
                                }
                            });
                        }
                        Err(err) => {
                            ui.label(format!("Cannot read: {}", err));
                        }
                    }
                });
        }
        system_state.apply(world);
    }
}