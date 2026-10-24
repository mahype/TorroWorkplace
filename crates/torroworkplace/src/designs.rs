//! The examples the suite ships (tasks L.5, L.7–L.9, L.12–L.16, L.20): `fonts/` and `color-schemes/`
//! in the repository, built into this binary and written to the same folders in the data folder
//! (`torro_core::design::{dir, font_dir, color_scheme_dir}`). Since issue #291 there are no example
//! designs: `designs/` in the repository holds the three built-in designs (Standard, Compact,
//! Console), which `torro-core` embeds and answers without a folder, so a designs folder only gets its
//! list of offered examples.
//!
//! They live here rather than in `torro-core`, because only the core service and the control
//! centre write these folders; in `torro-core` every app would carry the fonts (about 1 MB) without
//! ever using the copy.
//!
//! Each example is offered once (task L.10): every folder keeps the ids it was offered in a hidden
//! list, and a newer suite adds only the examples that are not on it. An example the user deleted or
//! changed stays deleted or changed, a folder of their own with an example's name stays theirs, and
//! an update of the suite never overwrites anything. Each of the three folders keeps its own list
//! (task L.20).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use torro_core::design::Part;

/// One file of an example: its path inside the example's folder and its bytes.
type File = (&'static str, &'static [u8]);

/// An example: its id (the folder name) and its files.
struct Example {
    id: &'static str,
    files: &'static [File],
}

/// The hidden file in each of the three folders that lists the examples the suite has offered there,
/// one id per line. The listings of `torro_core::design` read only folders, so it never shows up.
const OFFERED: &str = ".torro-examples";

/// The examples a designs folder from before the list (task L.10) was offered: only Plex existed.
const BEFORE_THE_LIST: &[&str] = &["plex"];

/// Builds the `(path, bytes)` pair of `<folder>/<id>/<path>` in the repository.
macro_rules! example_file {
    ($folder:literal, $id:literal, $path:literal) => {
        ($path, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../", $folder, "/", $id, "/", $path)))
    };
}

// The lists below hold every example with every file it has; the test
// `the_embedded_files_are_the_whole_example` fails when a file is added to the repository but not here.

/// The example designs: none since issue #291. The six of before (Plex, Commitr, Ribbon, Channelr,
/// Guildr, Quattro) are gone as designs, their fonts and colour schemes stay; the folders under
/// `designs/` in the repository are the built-in designs ([`torro_core::design::BUILT_IN_IDS`]).
const DESIGNS: &[Example] = &[];

/// The example fonts with every file they have (`fonts/` in the repository).
const FONTS: &[Example] = &[
    Example {
        id: "plex",
        files: &[
            example_file!("fonts", "plex", "font.toml"),
            example_file!("fonts", "plex", "IBMPlexSans-Bold.ttf"),
            example_file!("fonts", "plex", "IBMPlexSans-Medium.ttf"),
            example_file!("fonts", "plex", "IBMPlexSans-Regular.ttf"),
            example_file!("fonts", "plex", "IBMPlexSans-SemiBold.ttf"),
            example_file!("fonts", "plex", "LICENSE.txt"),
        ],
    },
    Example {
        id: "commitr",
        files: &[
            example_file!("fonts", "commitr", "font.toml"),
            example_file!("fonts", "commitr", "CommitrFigures-Medium.ttf"),
            example_file!("fonts", "commitr", "CommitrFigures-Regular.ttf"),
            example_file!("fonts", "commitr", "LICENSE.txt"),
            example_file!("fonts", "commitr", "MonaSans-Bold.ttf"),
            example_file!("fonts", "commitr", "MonaSans-Medium.ttf"),
            example_file!("fonts", "commitr", "MonaSans-Regular.ttf"),
            example_file!("fonts", "commitr", "MonaSans-SemiBold.ttf"),
        ],
    },
    Example {
        id: "ribbon",
        files: &[
            example_file!("fonts", "ribbon", "font.toml"),
            example_file!("fonts", "ribbon", "LICENSE.txt"),
            example_file!("fonts", "ribbon", "Selawik-Bold.ttf"),
            example_file!("fonts", "ribbon", "Selawik-Regular.ttf"),
            example_file!("fonts", "ribbon", "Selawik-SemiBold.ttf"),
        ],
    },
    Example {
        id: "channelr",
        files: &[
            example_file!("fonts", "channelr", "font.toml"),
            example_file!("fonts", "channelr", "LICENSE.txt"),
            example_file!("fonts", "channelr", "Lato-Bold.ttf"),
            example_file!("fonts", "channelr", "Lato-Regular.ttf"),
        ],
    },
    Example {
        id: "guildr",
        files: &[
            example_file!("fonts", "guildr", "font.toml"),
            example_file!("fonts", "guildr", "Figtree-Bold.ttf"),
            example_file!("fonts", "guildr", "Figtree-Medium.ttf"),
            example_file!("fonts", "guildr", "Figtree-Regular.ttf"),
            example_file!("fonts", "guildr", "Figtree-SemiBold.ttf"),
            example_file!("fonts", "guildr", "FigtreeTabular-Medium.ttf"),
            example_file!("fonts", "guildr", "FigtreeTabular-Regular.ttf"),
            example_file!("fonts", "guildr", "LICENSE.txt"),
        ],
    },
    Example {
        id: "quattro",
        files: &[
            example_file!("fonts", "quattro", "font.toml"),
            example_file!("fonts", "quattro", "LICENSE.txt"),
            example_file!("fonts", "quattro", "RobotoFlex-Bold.ttf"),
            example_file!("fonts", "quattro", "RobotoFlex-Medium.ttf"),
            example_file!("fonts", "quattro", "RobotoFlex-Regular.ttf"),
            example_file!("fonts", "quattro", "RobotoFlex-SemiBold.ttf"),
        ],
    },
    // Issue #299 (tasks SR.5-SR.7): the free fonts the suite ships, by id.
    Example {
        id: "cantarell",
        files: &[
            example_file!("fonts", "cantarell", "font.toml"),
            example_file!("fonts", "cantarell", "Cantarell-Bold.ttf"),
            example_file!("fonts", "cantarell", "Cantarell-Regular.ttf"),
            example_file!("fonts", "cantarell", "CantarellFigures-Regular.ttf"),
            example_file!("fonts", "cantarell", "LICENSE.txt"),
        ],
    },
    Example {
        id: "dm-sans",
        files: &[
            example_file!("fonts", "dm-sans", "font.toml"),
            example_file!("fonts", "dm-sans", "DMSans-Bold.ttf"),
            example_file!("fonts", "dm-sans", "DMSans-Medium.ttf"),
            example_file!("fonts", "dm-sans", "DMSans-Regular.ttf"),
            example_file!("fonts", "dm-sans", "DMSans-SemiBold.ttf"),
            example_file!("fonts", "dm-sans", "DMSansFigures-Medium.ttf"),
            example_file!("fonts", "dm-sans", "DMSansFigures-Regular.ttf"),
            example_file!("fonts", "dm-sans", "LICENSE.txt"),
        ],
    },
    Example {
        id: "fira-code",
        files: &[
            example_file!("fonts", "fira-code", "font.toml"),
            example_file!("fonts", "fira-code", "FiraCode-Bold.ttf"),
            example_file!("fonts", "fira-code", "FiraCode-Medium.ttf"),
            example_file!("fonts", "fira-code", "FiraCode-Regular.ttf"),
            example_file!("fonts", "fira-code", "FiraCode-SemiBold.ttf"),
            example_file!("fonts", "fira-code", "LICENSE.txt"),
        ],
    },
    Example {
        id: "fira-sans",
        files: &[
            example_file!("fonts", "fira-sans", "font.toml"),
            example_file!("fonts", "fira-sans", "FiraSans-Bold.ttf"),
            example_file!("fonts", "fira-sans", "FiraSans-Medium.ttf"),
            example_file!("fonts", "fira-sans", "FiraSans-Regular.ttf"),
            example_file!("fonts", "fira-sans", "FiraSans-SemiBold.ttf"),
            example_file!("fonts", "fira-sans", "FiraSansFigures-Medium.ttf"),
            example_file!("fonts", "fira-sans", "FiraSansFigures-Regular.ttf"),
            example_file!("fonts", "fira-sans", "LICENSE.txt"),
        ],
    },
    Example {
        id: "geist",
        files: &[
            example_file!("fonts", "geist", "font.toml"),
            example_file!("fonts", "geist", "Geist-Bold.ttf"),
            example_file!("fonts", "geist", "Geist-Medium.ttf"),
            example_file!("fonts", "geist", "Geist-Regular.ttf"),
            example_file!("fonts", "geist", "Geist-SemiBold.ttf"),
            example_file!("fonts", "geist", "GeistFigures-Medium.ttf"),
            example_file!("fonts", "geist", "GeistFigures-Regular.ttf"),
            example_file!("fonts", "geist", "LICENSE.txt"),
        ],
    },
    Example {
        id: "geist-mono",
        files: &[
            example_file!("fonts", "geist-mono", "font.toml"),
            example_file!("fonts", "geist-mono", "GeistMono-Bold.ttf"),
            example_file!("fonts", "geist-mono", "GeistMono-Medium.ttf"),
            example_file!("fonts", "geist-mono", "GeistMono-Regular.ttf"),
            example_file!("fonts", "geist-mono", "GeistMono-SemiBold.ttf"),
            example_file!("fonts", "geist-mono", "LICENSE.txt"),
        ],
    },
    Example {
        id: "hyperlegible",
        files: &[
            example_file!("fonts", "hyperlegible", "font.toml"),
            example_file!("fonts", "hyperlegible", "AtkinsonHyperlegibleNext-Bold.ttf"),
            example_file!("fonts", "hyperlegible", "AtkinsonHyperlegibleNext-Medium.ttf"),
            example_file!("fonts", "hyperlegible", "AtkinsonHyperlegibleNext-Regular.ttf"),
            example_file!("fonts", "hyperlegible", "AtkinsonHyperlegibleNext-SemiBold.ttf"),
            example_file!("fonts", "hyperlegible", "HyperlegibleFigures-Medium.ttf"),
            example_file!("fonts", "hyperlegible", "HyperlegibleFigures-Regular.ttf"),
            example_file!("fonts", "hyperlegible", "LICENSE.txt"),
        ],
    },
    Example {
        id: "inconsolata",
        files: &[
            example_file!("fonts", "inconsolata", "font.toml"),
            example_file!("fonts", "inconsolata", "Inconsolata-Bold.ttf"),
            example_file!("fonts", "inconsolata", "Inconsolata-Medium.ttf"),
            example_file!("fonts", "inconsolata", "Inconsolata-Regular.ttf"),
            example_file!("fonts", "inconsolata", "Inconsolata-SemiBold.ttf"),
            example_file!("fonts", "inconsolata", "LICENSE.txt"),
        ],
    },
    Example {
        id: "jetbrains-mono",
        files: &[
            example_file!("fonts", "jetbrains-mono", "font.toml"),
            example_file!("fonts", "jetbrains-mono", "JetBrainsMono-Bold.ttf"),
            example_file!("fonts", "jetbrains-mono", "JetBrainsMono-Medium.ttf"),
            example_file!("fonts", "jetbrains-mono", "JetBrainsMono-Regular.ttf"),
            example_file!("fonts", "jetbrains-mono", "JetBrainsMono-SemiBold.ttf"),
            example_file!("fonts", "jetbrains-mono", "LICENSE.txt"),
        ],
    },
    Example {
        id: "lexend",
        files: &[
            example_file!("fonts", "lexend", "font.toml"),
            example_file!("fonts", "lexend", "LICENSE.txt"),
            example_file!("fonts", "lexend", "Lexend-Bold.ttf"),
            example_file!("fonts", "lexend", "Lexend-Medium.ttf"),
            example_file!("fonts", "lexend", "Lexend-Regular.ttf"),
            example_file!("fonts", "lexend", "Lexend-SemiBold.ttf"),
            example_file!("fonts", "lexend", "LexendFigures-Medium.ttf"),
            example_file!("fonts", "lexend", "LexendFigures-Regular.ttf"),
        ],
    },
    Example {
        id: "literata",
        files: &[
            example_file!("fonts", "literata", "font.toml"),
            example_file!("fonts", "literata", "LICENSE.txt"),
            example_file!("fonts", "literata", "Literata-Bold.ttf"),
            example_file!("fonts", "literata", "Literata-Medium.ttf"),
            example_file!("fonts", "literata", "Literata-Regular.ttf"),
            example_file!("fonts", "literata", "Literata-SemiBold.ttf"),
            example_file!("fonts", "literata", "LiterataFigures-Medium.ttf"),
            example_file!("fonts", "literata", "LiterataFigures-Regular.ttf"),
        ],
    },
    Example {
        id: "lora",
        files: &[
            example_file!("fonts", "lora", "font.toml"),
            example_file!("fonts", "lora", "LICENSE.txt"),
            example_file!("fonts", "lora", "Lora-Bold.ttf"),
            example_file!("fonts", "lora", "Lora-Medium.ttf"),
            example_file!("fonts", "lora", "Lora-Regular.ttf"),
            example_file!("fonts", "lora", "Lora-SemiBold.ttf"),
            example_file!("fonts", "lora", "TorroQuillFigures-Medium.ttf"),
            example_file!("fonts", "lora", "TorroQuillFigures-Regular.ttf"),
        ],
    },
    Example {
        id: "manrope",
        files: &[
            example_file!("fonts", "manrope", "font.toml"),
            example_file!("fonts", "manrope", "LICENSE.txt"),
            example_file!("fonts", "manrope", "Manrope-Bold.ttf"),
            example_file!("fonts", "manrope", "Manrope-Medium.ttf"),
            example_file!("fonts", "manrope", "Manrope-Regular.ttf"),
            example_file!("fonts", "manrope", "Manrope-SemiBold.ttf"),
            example_file!("fonts", "manrope", "ManropeFigures-Medium.ttf"),
            example_file!("fonts", "manrope", "ManropeFigures-Regular.ttf"),
        ],
    },
    Example {
        id: "merriweather",
        files: &[
            example_file!("fonts", "merriweather", "font.toml"),
            example_file!("fonts", "merriweather", "LICENSE.txt"),
            example_file!("fonts", "merriweather", "Merriweather-Bold.ttf"),
            example_file!("fonts", "merriweather", "Merriweather-Medium.ttf"),
            example_file!("fonts", "merriweather", "Merriweather-Regular.ttf"),
            example_file!("fonts", "merriweather", "Merriweather-SemiBold.ttf"),
            example_file!("fonts", "merriweather", "TorroSerifFigures-Medium.ttf"),
            example_file!("fonts", "merriweather", "TorroSerifFigures-Regular.ttf"),
        ],
    },
    Example {
        id: "newsreader",
        files: &[
            example_file!("fonts", "newsreader", "font.toml"),
            example_file!("fonts", "newsreader", "LICENSE.txt"),
            example_file!("fonts", "newsreader", "Newsreader16pt-Bold.ttf"),
            example_file!("fonts", "newsreader", "Newsreader16pt-Medium.ttf"),
            example_file!("fonts", "newsreader", "Newsreader16pt-Regular.ttf"),
            example_file!("fonts", "newsreader", "Newsreader16pt-SemiBold.ttf"),
        ],
    },
    Example {
        id: "noto-sans",
        files: &[
            example_file!("fonts", "noto-sans", "font.toml"),
            example_file!("fonts", "noto-sans", "LICENSE.txt"),
            example_file!("fonts", "noto-sans", "NotoSans-Bold.ttf"),
            example_file!("fonts", "noto-sans", "NotoSans-Medium.ttf"),
            example_file!("fonts", "noto-sans", "NotoSans-Regular.ttf"),
            example_file!("fonts", "noto-sans", "NotoSans-SemiBold.ttf"),
        ],
    },
    Example {
        id: "nunito",
        files: &[
            example_file!("fonts", "nunito", "font.toml"),
            example_file!("fonts", "nunito", "LICENSE.txt"),
            example_file!("fonts", "nunito", "Nunito-Bold.ttf"),
            example_file!("fonts", "nunito", "Nunito-Medium.ttf"),
            example_file!("fonts", "nunito", "Nunito-Regular.ttf"),
            example_file!("fonts", "nunito", "Nunito-SemiBold.ttf"),
        ],
    },
    Example {
        id: "open-sans",
        files: &[
            example_file!("fonts", "open-sans", "font.toml"),
            example_file!("fonts", "open-sans", "LICENSE.txt"),
            example_file!("fonts", "open-sans", "OpenSans-Bold.ttf"),
            example_file!("fonts", "open-sans", "OpenSans-Regular.ttf"),
            example_file!("fonts", "open-sans", "OpenSans-SemiBold.ttf"),
        ],
    },
    Example {
        id: "plex-mono",
        files: &[
            example_file!("fonts", "plex-mono", "font.toml"),
            example_file!("fonts", "plex-mono", "IBMPlexMono-Bold.ttf"),
            example_file!("fonts", "plex-mono", "IBMPlexMono-Medium.ttf"),
            example_file!("fonts", "plex-mono", "IBMPlexMono-Regular.ttf"),
            example_file!("fonts", "plex-mono", "IBMPlexMono-SemiBold.ttf"),
            example_file!("fonts", "plex-mono", "LICENSE.txt"),
        ],
    },
    Example {
        id: "public-sans",
        files: &[
            example_file!("fonts", "public-sans", "font.toml"),
            example_file!("fonts", "public-sans", "LICENSE.txt"),
            example_file!("fonts", "public-sans", "PublicSans-Bold.ttf"),
            example_file!("fonts", "public-sans", "PublicSans-Medium.ttf"),
            example_file!("fonts", "public-sans", "PublicSans-Regular.ttf"),
            example_file!("fonts", "public-sans", "PublicSans-SemiBold.ttf"),
            example_file!("fonts", "public-sans", "PublicSansFigures-Medium.ttf"),
            example_file!("fonts", "public-sans", "PublicSansFigures-Regular.ttf"),
        ],
    },
    Example {
        id: "roboto",
        files: &[
            example_file!("fonts", "roboto", "font.toml"),
            example_file!("fonts", "roboto", "LICENSE.txt"),
            example_file!("fonts", "roboto", "Roboto-Bold.ttf"),
            example_file!("fonts", "roboto", "Roboto-Medium.ttf"),
            example_file!("fonts", "roboto", "Roboto-Regular.ttf"),
        ],
    },
    Example {
        id: "rubik",
        files: &[
            example_file!("fonts", "rubik", "font.toml"),
            example_file!("fonts", "rubik", "LICENSE.txt"),
            example_file!("fonts", "rubik", "Rubik-Bold.ttf"),
            example_file!("fonts", "rubik", "Rubik-Medium.ttf"),
            example_file!("fonts", "rubik", "Rubik-Regular.ttf"),
            example_file!("fonts", "rubik", "Rubik-SemiBold.ttf"),
            example_file!("fonts", "rubik", "RubikFigures-Medium.ttf"),
            example_file!("fonts", "rubik", "RubikFigures-Regular.ttf"),
        ],
    },
    Example {
        id: "source",
        files: &[
            example_file!("fonts", "source", "font.toml"),
            example_file!("fonts", "source", "LICENSE.txt"),
            example_file!("fonts", "source", "SourceSans3-Bold.ttf"),
            example_file!("fonts", "source", "SourceSans3-Medium.ttf"),
            example_file!("fonts", "source", "SourceSans3-Regular.ttf"),
            example_file!("fonts", "source", "SourceSans3-SemiBold.ttf"),
            example_file!("fonts", "source", "SourceSerif4-Regular.ttf"),
            example_file!("fonts", "source", "SourceSerif4-SemiBold.ttf"),
        ],
    },
    Example {
        id: "source-code-pro",
        files: &[
            example_file!("fonts", "source-code-pro", "font.toml"),
            example_file!("fonts", "source-code-pro", "LICENSE.txt"),
            example_file!("fonts", "source-code-pro", "SourceCodePro-Bold.ttf"),
            example_file!("fonts", "source-code-pro", "SourceCodePro-Medium.ttf"),
            example_file!("fonts", "source-code-pro", "SourceCodePro-Regular.ttf"),
            example_file!("fonts", "source-code-pro", "SourceCodePro-SemiBold.ttf"),
        ],
    },
    Example {
        id: "work-sans",
        files: &[
            example_file!("fonts", "work-sans", "font.toml"),
            example_file!("fonts", "work-sans", "LICENSE.txt"),
            example_file!("fonts", "work-sans", "WorkSans-Bold.ttf"),
            example_file!("fonts", "work-sans", "WorkSans-Medium.ttf"),
            example_file!("fonts", "work-sans", "WorkSans-Regular.ttf"),
            example_file!("fonts", "work-sans", "WorkSans-SemiBold.ttf"),
            example_file!("fonts", "work-sans", "WorkSansFigures-Medium.ttf"),
            example_file!("fonts", "work-sans", "WorkSansFigures-Regular.ttf"),
        ],
    },
];

/// The example colour schemes with every file they have (`color-schemes/` in the repository).
const COLOR_SCHEMES: &[Example] = &[
    Example {
        id: "plex",
        files: &[
            example_file!("color-schemes", "plex", "color-scheme.toml"),
            example_file!("color-schemes", "plex", "logo.svg"),
        ],
    },
    Example {
        id: "commitr",
        files: &[
            example_file!("color-schemes", "commitr", "color-scheme.toml"),
            example_file!("color-schemes", "commitr", "logo.svg"),
        ],
    },
    Example {
        id: "ribbon",
        files: &[
            example_file!("color-schemes", "ribbon", "color-scheme.toml"),
            example_file!("color-schemes", "ribbon", "logo.svg"),
        ],
    },
    Example {
        id: "channelr",
        files: &[
            example_file!("color-schemes", "channelr", "color-scheme.toml"),
            example_file!("color-schemes", "channelr", "logo.svg"),
        ],
    },
    Example {
        id: "guildr",
        files: &[
            example_file!("color-schemes", "guildr", "color-scheme.toml"),
            example_file!("color-schemes", "guildr", "logo.svg"),
        ],
    },
    Example {
        id: "quattro",
        files: &[
            example_file!("color-schemes", "quattro", "color-scheme.toml"),
            example_file!("color-schemes", "quattro", "logo.svg"),
        ],
    },
    Example {
        id: "nord",
        files: &[
            example_file!("color-schemes", "nord", "color-scheme.toml"),
            example_file!("color-schemes", "nord", "LICENSE.txt"),
            example_file!("color-schemes", "nord", "logo.svg"),
        ],
    },
    Example {
        id: "solarized",
        files: &[
            example_file!("color-schemes", "solarized", "color-scheme.toml"),
            example_file!("color-schemes", "solarized", "LICENSE.txt"),
            example_file!("color-schemes", "solarized", "logo.svg"),
        ],
    },
    Example {
        id: "catppuccin",
        files: &[
            example_file!("color-schemes", "catppuccin", "color-scheme.toml"),
            example_file!("color-schemes", "catppuccin", "LICENSE.txt"),
            example_file!("color-schemes", "catppuccin", "logo.svg"),
        ],
    },
    Example {
        id: "gruvbox",
        files: &[
            example_file!("color-schemes", "gruvbox", "color-scheme.toml"),
            example_file!("color-schemes", "gruvbox", "LICENSE.txt"),
            example_file!("color-schemes", "gruvbox", "logo.svg"),
        ],
    },
    Example {
        id: "dracula",
        files: &[
            example_file!("color-schemes", "dracula", "color-scheme.toml"),
            example_file!("color-schemes", "dracula", "LICENSE.txt"),
            example_file!("color-schemes", "dracula", "logo.svg"),
        ],
    },
    Example {
        id: "everforest",
        files: &[
            example_file!("color-schemes", "everforest", "color-scheme.toml"),
            example_file!("color-schemes", "everforest", "LICENSE.txt"),
            example_file!("color-schemes", "everforest", "logo.svg"),
        ],
    },
    Example {
        id: "ayu",
        files: &[
            example_file!("color-schemes", "ayu", "color-scheme.toml"),
            example_file!("color-schemes", "ayu", "LICENSE.txt"),
        ],
    },
    Example {
        id: "base16",
        files: &[
            example_file!("color-schemes", "base16", "color-scheme.toml"),
            example_file!("color-schemes", "base16", "LICENSE.txt"),
        ],
    },
    Example {
        id: "edge",
        files: &[
            example_file!("color-schemes", "edge", "color-scheme.toml"),
            example_file!("color-schemes", "edge", "LICENSE.txt"),
        ],
    },
    Example {
        id: "flexoki",
        files: &[
            example_file!("color-schemes", "flexoki", "color-scheme.toml"),
            example_file!("color-schemes", "flexoki", "LICENSE.txt"),
        ],
    },
    Example {
        id: "iceberg",
        files: &[
            example_file!("color-schemes", "iceberg", "color-scheme.toml"),
            example_file!("color-schemes", "iceberg", "LICENSE.txt"),
        ],
    },
    Example {
        id: "kanagawa",
        files: &[
            example_file!("color-schemes", "kanagawa", "color-scheme.toml"),
            example_file!("color-schemes", "kanagawa", "LICENSE.txt"),
        ],
    },
    Example {
        id: "night-owl",
        files: &[
            example_file!("color-schemes", "night-owl", "color-scheme.toml"),
            example_file!("color-schemes", "night-owl", "LICENSE.txt"),
        ],
    },
    Example {
        id: "nightfox",
        files: &[
            example_file!("color-schemes", "nightfox", "color-scheme.toml"),
            example_file!("color-schemes", "nightfox", "LICENSE.txt"),
        ],
    },
    Example {
        id: "one",
        files: &[
            example_file!("color-schemes", "one", "color-scheme.toml"),
            example_file!("color-schemes", "one", "LICENSE.txt"),
        ],
    },
    Example {
        id: "primer",
        files: &[
            example_file!("color-schemes", "primer", "color-scheme.toml"),
            example_file!("color-schemes", "primer", "LICENSE.txt"),
        ],
    },
    Example {
        id: "rose-pine",
        files: &[
            example_file!("color-schemes", "rose-pine", "color-scheme.toml"),
            example_file!("color-schemes", "rose-pine", "LICENSE.txt"),
        ],
    },
    Example {
        id: "selenized",
        files: &[
            example_file!("color-schemes", "selenized", "color-scheme.toml"),
            example_file!("color-schemes", "selenized", "LICENSE.txt"),
        ],
    },
    Example {
        id: "slate",
        files: &[
            example_file!("color-schemes", "slate", "color-scheme.toml"),
            example_file!("color-schemes", "slate", "LICENSE.txt"),
        ],
    },
    Example {
        id: "tango",
        files: &[
            example_file!("color-schemes", "tango", "color-scheme.toml"),
            example_file!("color-schemes", "tango", "LICENSE.txt"),
        ],
    },
    Example {
        id: "tokyo-night",
        files: &[
            example_file!("color-schemes", "tokyo-night", "color-scheme.toml"),
            example_file!("color-schemes", "tokyo-night", "LICENSE.txt"),
        ],
    },
    Example {
        id: "tomorrow",
        files: &[
            example_file!("color-schemes", "tomorrow", "color-scheme.toml"),
            example_file!("color-schemes", "tomorrow", "LICENSE.txt"),
        ],
    },
    Example {
        id: "cobalt2",
        files: &[
            example_file!("color-schemes", "cobalt2", "color-scheme.toml"),
            example_file!("color-schemes", "cobalt2", "LICENSE.txt"),
        ],
    },
    Example {
        id: "palenight",
        files: &[
            example_file!("color-schemes", "palenight", "color-scheme.toml"),
            example_file!("color-schemes", "palenight", "LICENSE.txt"),
        ],
    },
    Example {
        id: "poimandres",
        files: &[
            example_file!("color-schemes", "poimandres", "color-scheme.toml"),
            example_file!("color-schemes", "poimandres", "LICENSE.txt"),
        ],
    },
    Example {
        id: "sonokai",
        files: &[
            example_file!("color-schemes", "sonokai", "color-scheme.toml"),
            example_file!("color-schemes", "sonokai", "LICENSE.txt"),
        ],
    },
    Example {
        id: "synthwave-84",
        files: &[
            example_file!("color-schemes", "synthwave-84", "color-scheme.toml"),
            example_file!("color-schemes", "synthwave-84", "LICENSE.txt"),
        ],
    },
];

/// Whether the font `id` ships with the suite (issue #299, `FontInfo::origin`): the standard or one of
/// the example fonts laid into the fonts folder. A folder of that name the user changed since still
/// counts: the id is what the control centre groups by.
pub fn bundled_font(id: &str) -> bool {
    id == torro_core::design::STANDARD_ID || FONTS.iter().any(|example| example.id == id)
}

/// The examples of `part`.
fn examples(part: Part) -> &'static [Example] {
    match part {
        Part::Design => DESIGNS,
        Part::Font => FONTS,
        Part::ColorScheme => COLOR_SCHEMES,
    }
}

/// The three folders of the look in one data folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    /// `designs/`.
    pub designs: PathBuf,
    /// `fonts/`.
    pub fonts: PathBuf,
    /// `color-schemes/`.
    pub color_schemes: PathBuf,
}

impl Folders {
    /// The user's folders (`torro_core::design::{dir, font_dir, color_scheme_dir}`); `None` without a
    /// home directory.
    pub fn user() -> Option<Folders> {
        Some(Folders {
            designs: torro_core::design::dir()?,
            fonts: torro_core::design::font_dir()?,
            color_schemes: torro_core::design::color_scheme_dir()?,
        })
    }

    /// The folder of `part`.
    pub fn get(&self, part: Part) -> &Path {
        match part {
            Part::Design => &self.designs,
            Part::Font => &self.fonts,
            Part::ColorScheme => &self.color_schemes,
        }
    }
}

/// The three parts, in the order they are brought up to date.
const PARTS: [Part; 3] = [Part::Design, Part::Font, Part::ColorScheme];

/// Brings all three `folders` up to the examples of this suite ([`install_new`] for each), each on
/// its own: an error in one does not keep the others from their examples. Returns every error with
/// its part.
pub fn install_all(folders: &Folders) -> Vec<(Part, io::Error)> {
    PARTS.into_iter().filter_map(|part| install_new(folders.get(part), part).err().map(|err| (part, err))).collect()
}

/// Brings the folder `dir` of `part` up to this suite's examples of that part: a missing folder is
/// created with all of them, an existing one gets each example it was not offered yet, unless a
/// folder of that name is already there. Returns the ids it wrote. Each folder keeps its own list.
///
/// Every example is written to a hidden folder and moved into place in one step, the list too, so a
/// start that is cut short leaves nothing half done; an example another process put there in the
/// meantime counts as written.
pub fn install_new(dir: &Path, part: Part) -> io::Result<Vec<&'static str>> {
    if fs::symlink_metadata(dir).is_err() {
        return create(dir, part);
    }
    let (mut offered, known) = offered_so_far(dir, part)?;
    let mut added = Vec::new();
    let mut changed = !known;
    for example in examples(part) {
        if offered.iter().any(|id| id == example.id) {
            continue;
        }
        let target = dir.join(example.id);
        if fs::symlink_metadata(&target).is_err() {
            add_example(dir, &target, example)?;
            added.push(example.id);
        }
        offered.push(example.id.to_string());
        changed = true;
    }
    if changed {
        write_list(dir, &offered)?;
    }
    Ok(added)
}

/// What the existing folder `dir` (of `part`) was offered so far, and whether it has a list. Without
/// a list: for designs the examples of before the list and every example whose folder is there (an
/// older suite or the user put it there); for fonts and colour schemes nothing.
fn offered_so_far(dir: &Path, part: Part) -> io::Result<(Vec<String>, bool)> {
    match fs::read_to_string(dir.join(OFFERED)) {
        Ok(text) => Ok((text.lines().map(str::trim).filter(|id| !id.is_empty()).map(String::from).collect(), true)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            Ok((if part == Part::Design { before_the_list(dir) } else { Vec::new() }, false))
        }
        Err(err) => Err(err),
    }
}

/// What a designs folder without the list was offered: the examples of before the list, and every
/// example whose folder is there.
fn before_the_list(dir: &Path) -> Vec<String> {
    let present = DESIGNS.iter().map(|example| example.id).filter(|id| dir.join(id).exists());
    let present = present.filter(|id| !BEFORE_THE_LIST.contains(id));
    BEFORE_THE_LIST.iter().copied().chain(present).map(String::from).collect()
}

/// Creates the missing folder `dir` with every example of `part` and the list, through a hidden
/// folder beside it moved into place in one step: a second process starting at the same time finds
/// either no folder or the whole one.
fn create(dir: &Path, part: Part) -> io::Result<Vec<&'static str>> {
    let parent = dir.parent().ok_or_else(|| io::Error::other("the folder has no parent"))?;
    fs::create_dir_all(parent)?;
    let name = dir.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let staging = parent.join(format!(".{name}-{}", std::process::id()));
    let ids: Vec<&'static str> = examples(part).iter().map(|example| example.id).collect();
    let offered: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
    let written = examples(part)
        .iter()
        .try_for_each(|example| write_example(&staging.join(example.id), example))
        .and_then(|()| fs::create_dir_all(&staging))
        .and_then(|()| fs::write(staging.join(OFFERED), list_text(&offered)))
        .and_then(|()| fs::rename(&staging, dir));
    if let Err(err) = written {
        let _ = fs::remove_dir_all(&staging);
        // Another process put the folder there in the meantime: it has the examples.
        return if dir.is_dir() { Ok(Vec::new()) } else { Err(err) };
    }
    Ok(ids)
}

/// Writes the files of `example` into the folder `to`.
fn write_example(to: &Path, example: &Example) -> io::Result<()> {
    for (path, bytes) in example.files {
        let target = to.join(path);
        if let Some(folder) = target.parent() {
            fs::create_dir_all(folder)?;
        }
        fs::write(target, bytes)?;
    }
    Ok(())
}

/// Writes `example` as the folder `target` in `dir`, through a hidden folder beside it.
fn add_example(dir: &Path, target: &Path, example: &Example) -> io::Result<()> {
    let staging = dir.join(format!(".example-{}-{}", example.id, std::process::id()));
    if let Err(err) = write_example(&staging, example).and_then(|()| fs::rename(&staging, target)) {
        let _ = fs::remove_dir_all(&staging);
        // Another process put the example there in the meantime.
        if !target.is_dir() {
            return Err(err);
        }
    }
    Ok(())
}

/// Replaces the list of `dir` in one step.
fn write_list(dir: &Path, ids: &[String]) -> io::Result<()> {
    let staging = dir.join(format!("{OFFERED}.{}", std::process::id()));
    fs::write(&staging, list_text(ids))?;
    fs::rename(&staging, dir.join(OFFERED))
}

/// The list file's text: one id per line.
fn list_text(ids: &[String]) -> String {
    ids.iter().map(|id| format!("{id}\n")).collect()
}

/// [`install_all`] for the user's folders, where errors are only reported: the suite works without
/// the examples. Called before the control centre opens a folder; it never carries old packs over,
/// so only the core service ever writes `designs/.torro-migrated`.
pub fn install_examples() {
    let Some(folders) = Folders::user() else { return };
    report(&folders, install_all(&folders));
}

/// Prints each error with the folder it happened in.
fn report(folders: &Folders, errors: Vec<(Part, io::Error)>) {
    for (part, err) in errors {
        eprintln!("torroworkplace: cannot write the examples to {}: {err}", folders.get(part).display());
    }
}

#[cfg(test)]
mod tests {
    use torro_core::design::{
        self, BUILT_IN_IDS, ColorScheme, Design, DesignError, LevelName, Rgba, STANDARD_ID, Selection, Weight,
    };

    use super::*;

    /// `<folder>/` of `part` in the repository.
    fn repo(part: Part) -> PathBuf {
        let folder = match part {
            Part::Design => "designs",
            Part::Font => "fonts",
            Part::ColorScheme => "color-schemes",
        };
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(folder)
    }

    /// The three folders under `data`.
    fn folders(data: &Path) -> Folders {
        Folders { designs: data.join("designs"), fonts: data.join("fonts"), color_schemes: data.join("color-schemes") }
    }

    /// The font and colour scheme `id` of the repository, each from its own folder, with the standard
    /// design (the examples are no designs since issue #291).
    fn shipped(id: &str) -> Selection {
        let fail = |part: &str, problems: Vec<DesignError>| -> ! {
            let reasons: Vec<String> = problems.iter().map(ToString::to_string).collect();
            panic!("{part}/{id} must be valid: {reasons:#?}")
        };
        Selection {
            design: Design::standard(),
            font: design::load_font(&repo(Part::Font), id).unwrap_or_else(|p| fail("fonts", p)),
            scheme: design::load_color_scheme(&repo(Part::ColorScheme), id)
                .unwrap_or_else(|p| fail("color-schemes", p)),
        }
    }

    /// Every file under `dir`, relative, with `/` between folders.
    fn files_in(dir: &Path, prefix: &str, found: &mut Vec<String>) {
        for entry in fs::read_dir(dir).expect("read folder").flatten() {
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                files_in(&entry.path(), &format!("{name}/"), found);
            } else {
                found.push(name);
            }
        }
    }

    /// The ids in the list of `dir`.
    fn offered(dir: &Path) -> Vec<String> {
        fs::read_to_string(dir.join(OFFERED)).expect("list").lines().map(String::from).collect()
    }

    fn ids(part: Part) -> Vec<String> {
        examples(part).iter().map(|example| example.id.to_string()).collect()
    }

    /// Every entry of `dir`, sorted.
    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(dir).expect("read").flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    /// Every file in the three folders, for "nothing changed".
    fn everything(all: &Folders) -> Vec<(String, Vec<u8>)> {
        let mut found = Vec::new();
        for part in PARTS {
            let mut names = Vec::new();
            files_in(all.get(part), &format!("{part:?}/"), &mut names);
            let dir = all.get(part).to_path_buf();
            found.extend(names.into_iter().map(|name| {
                let rel = name.split_once('/').map_or(name.as_str(), |(_, rel)| rel).to_string();
                let bytes = fs::read(dir.join(&rel)).expect("read");
                (name, bytes)
            }));
        }
        found.sort();
        found
    }

    /// The six examples that are a font and a colour scheme (and were a design too until issue #291).
    const COMPLETE: [&str; 6] = ["channelr", "commitr", "guildr", "plex", "quattro", "ribbon"];
    /// The fonts that are only a font (issue #299, tasks SR.5–SR.7), by id.
    const FONTS_ONLY: [&str; 25] = [
        "cantarell",
        "dm-sans",
        "fira-code",
        "fira-sans",
        "geist",
        "geist-mono",
        "hyperlegible",
        "inconsolata",
        "jetbrains-mono",
        "lexend",
        "literata",
        "lora",
        "manrope",
        "merriweather",
        "newsreader",
        "noto-sans",
        "nunito",
        "open-sans",
        "plex-mono",
        "public-sans",
        "roboto",
        "rubik",
        "source",
        "source-code-pro",
        "work-sans",
    ];
    /// The six examples that are only a colour scheme (task L.20).
    const SCHEMES_ONLY: [&str; 6] = ["catppuccin", "dracula", "everforest", "gruvbox", "nord", "solarized"];
    /// The colour schemes after free palettes in light and dark (issue #292). They bring no logo of their
    /// own, as the dark-only ones: the suite draws its "T" on their accent.
    const WITHOUT_LOGO: [&str; 16] = [
        "ayu",
        "base16",
        "edge",
        "flexoki",
        "iceberg",
        "kanagawa",
        "night-owl",
        "nightfox",
        "one",
        "primer",
        "rose-pine",
        "selenized",
        "slate",
        "tango",
        "tokyo-night",
        "tomorrow",
    ];
    /// The colour schemes after free palettes that are only dark (issue #292, `dark_only`).
    const DARK_ONLY: [&str; 5] = ["cobalt2", "palenight", "poimandres", "sonokai", "synthwave-84"];

    #[test]
    fn the_embedded_files_are_the_whole_example() {
        for part in PARTS {
            let mut folders = entries(&repo(part));
            folders.retain(|name| repo(part).join(name).is_dir());
            // The built-in designs are embedded in `torro-core`, not offered as examples (issue #291).
            if part == Part::Design {
                let mut built_in = BUILT_IN_IDS.map(String::from).to_vec();
                built_in.sort();
                assert_eq!(folders, built_in, "designs/ holds the built-in designs only");
                folders.clear();
            }
            let mut expected = ids(part);
            expected.sort();
            assert_eq!(folders, expected, "every folder in {} is an example", repo(part).display());
            for example in examples(part) {
                let mut on_disk = Vec::new();
                files_in(&repo(part).join(example.id), "", &mut on_disk);
                on_disk.sort();
                let mut embedded: Vec<String> = example.files.iter().map(|(path, _)| path.to_string()).collect();
                embedded.sort();
                assert_eq!(embedded, on_disk, "{part:?} {}", example.id);
                assert!(!on_disk.iter().any(|file| file.ends_with("tray.svg")), "no tray symbol any more (#114)");
            }
        }
    }

    /// Task L.20: Nord and the other five are only colour schemes; the six others are a font and a
    /// colour scheme (no design since issue #291), and every one of them is valid. The schemes after
    /// free palettes (issue #292) are valid too and have no logo.
    #[test]
    fn every_example_loads_without_reasons_in_its_folders() {
        let listed = |part: Part| -> Vec<String> {
            let found: Vec<(String, Vec<DesignError>)> = match part {
                Part::Design => Vec::new(),
                Part::Font => {
                    design::list_fonts(&repo(part)).into_iter().map(|l| (l.id.clone(), l.problems().to_vec())).collect()
                }
                Part::ColorScheme => design::list_color_schemes(&repo(part))
                    .into_iter()
                    .map(|l| (l.id.clone(), l.problems().to_vec()))
                    .collect(),
            };
            for (id, problems) in &found {
                assert!(problems.is_empty(), "{part:?} {id}: {problems:?}");
            }
            found.into_iter().map(|(id, _)| id).filter(|id| id != STANDARD_ID).collect()
        };
        assert!(examples(Part::Design).is_empty(), "no example designs since issue #291");
        let mut fonts: Vec<&str> = COMPLETE.iter().chain(&FONTS_ONLY).copied().collect();
        fonts.sort_unstable();
        assert_eq!(listed(Part::Font), fonts);
        let mut schemes: Vec<&str> =
            COMPLETE.iter().chain(&SCHEMES_ONLY).chain(&WITHOUT_LOGO).chain(&DARK_ONLY).copied().collect();
        schemes.sort_unstable();
        assert_eq!(listed(Part::ColorScheme), schemes);
        for id in COMPLETE {
            let gone = design::load(&repo(Part::Design), id);
            assert!(matches!(&gone, Err(p) if matches!(p[..], [DesignError::NotFound { .. }])), "{id}: {gone:?}");
        }
        for id in COMPLETE.iter().chain(&SCHEMES_ONLY) {
            assert!(design::load_color_scheme(&repo(Part::ColorScheme), id).expect("valid").logo.mark.is_some());
        }
        for id in WITHOUT_LOGO.iter().chain(&DARK_ONLY) {
            let scheme = design::load_color_scheme(&repo(Part::ColorScheme), id).expect("valid");
            assert!(scheme.logo.mark.is_none(), "{id} draws the suite's T on its accent");
        }
    }

    /// Task FS.4: the five dark-only schemes are `dark_only`, every other shipped one has both modes.
    #[test]
    fn only_the_five_dark_schemes_are_dark_only() {
        let dark_only: Vec<String> = design::list_color_schemes(&repo(Part::ColorScheme))
            .into_iter()
            .filter(|listed| listed.result.as_ref().is_ok_and(|scheme| scheme.dark_only))
            .map(|listed| listed.id)
            .collect();
        assert_eq!(dark_only, DARK_ONLY);
        for id in DARK_ONLY {
            let scheme = design::load_color_scheme(&repo(Part::ColorScheme), id).expect("valid");
            assert_eq!(scheme.colors.light, scheme.colors.dark, "{id}: light mode draws the dark colours");
        }
    }

    /// The size adjustments the samples were measured with in task L.19.
    #[test]
    fn the_fonts_adjust_as_measured() {
        for (id, factor) in [
            ("plex", 1.058),
            ("commitr", 1.038),
            ("channelr", 1.078),
            ("guildr", 1.092),
            ("quattro", 1.063),
            ("ribbon", 1.092),
        ] {
            let font = design::load_font(&repo(Part::Font), id).expect("valid");
            assert!((font.size_adjust - factor).abs() < 0.001, "{id}: {}", font.size_adjust);
        }
    }

    #[test]
    fn the_plex_example_is_its_font_and_colour_scheme() {
        let look = shipped("plex");
        assert_eq!((look.font.about.name.as_str(), look.scheme.about.name.as_str()), ("Plex", "Plex"));
        assert_eq!(look.font.fonts.text.family, "IBM Plex Sans");
        assert_eq!(look.font.fonts.numbers, look.font.fonts.text, "Plex's figures are tabular already");
        let body = look.level(LevelName::Body);
        assert_eq!((body.size, body.line), (15.0, 22.0), "the standard's 14/22 in IBM Plex Sans: 14 × 1.058");
        assert_eq!(look.scheme.colors.dark.accent, Rgba::hex(0x3AA6A0));
        assert!(look.scheme.logo.mark.is_some());
    }

    #[test]
    fn the_commitr_example_is_its_font_and_colour_scheme() {
        let look = shipped("commitr");
        assert_eq!(look.font.fonts.text.family, "Mona Sans");
        assert_eq!(look.font.fonts.numbers.family, "Commitr Figures", "Mona Sans's figures, made tabular");
        let light = look.scheme.colors.light;
        assert!(light.own_sidebar() && !look.scheme.colors.dark.own_sidebar(), "a dark sidebar in light mode only");
        assert_eq!(light.accent, Rgba::hex(0x1F883D));
    }

    #[test]
    fn the_ribbon_example_is_its_font_and_colour_scheme() {
        let look = shipped("ribbon");
        assert_eq!(look.font.fonts.text.family, "Selawik");
        assert!(look.font.fonts.text.medium.is_none(), "Selawik has no Medium");
        let weight = |name| look.level(name).weight;
        assert_eq!((weight(LevelName::Caption), weight(LevelName::BodyStrong)), (Weight::Semibold, Weight::Semibold));
        assert!(!look.scheme.colors.light.own_sidebar() && !look.scheme.colors.dark.own_sidebar());
    }

    #[test]
    fn the_further_fonts_are_their_families() {
        // id, text font, figures font
        for (id, text, numbers) in [
            ("channelr", "Lato", "Lato"),
            ("guildr", "Figtree", "Figtree Tabular"),
            ("quattro", "Roboto Flex", "Roboto Flex"),
            // Issue #299 (SR.5): figures of one width stay the text font, else a figures font of its own.
            ("hyperlegible", "Atkinson Hyperlegible Next", "Hyperlegible Figures"),
            ("source", "Source Sans 3", "Source Sans 3"),
            ("nunito", "Nunito", "Nunito"),
            ("open-sans", "Open Sans", "Open Sans"),
            ("noto-sans", "Noto Sans", "Noto Sans"),
            ("roboto", "Roboto", "Roboto"),
            ("work-sans", "Work Sans", "Work Sans Figures"),
            ("manrope", "Manrope", "Manrope Figures"),
            // SR.6.
            ("dm-sans", "DM Sans", "DM Sans Figures"),
            ("public-sans", "Public Sans", "Public Sans Figures"),
            ("lexend", "Lexend", "Lexend Figures"),
            ("fira-sans", "Fira Sans", "Fira Sans Figures"),
            ("geist", "Geist", "Geist Figures"),
            ("rubik", "Rubik", "Rubik Figures"),
            ("cantarell", "Cantarell", "Cantarell Figures"),
            // SR.7: Merriweather and Lora have a Reserved Font Name, so their changed figures are named apart.
            ("literata", "Literata", "Literata Figures"),
            ("merriweather", "Merriweather", "Torro Serif Figures"),
            ("lora", "Lora", "Torro Quill Figures"),
            ("newsreader", "Newsreader 16pt", "Newsreader 16pt"),
            ("jetbrains-mono", "JetBrains Mono", "JetBrains Mono"),
            ("fira-code", "Fira Code", "Fira Code"),
            ("plex-mono", "IBM Plex Mono", "IBM Plex Mono"),
            ("source-code-pro", "Source Code Pro", "Source Code Pro"),
            ("geist-mono", "Geist Mono", "Geist Mono"),
            ("inconsolata", "Inconsolata", "Inconsolata"),
        ] {
            let font = design::load_font(&repo(Part::Font), id).unwrap_or_else(|p| panic!("{id}: {p:?}"));
            assert_eq!((font.fonts.text.family.as_str(), font.fonts.numbers.family.as_str()), (text, numbers));
        }
        assert_eq!(shipped("channelr").level(LevelName::Title).weight, Weight::Bold, "Lato has no SemiBold");
        let source = design::load_font(&repo(Part::Font), "source").expect("valid");
        assert_eq!(source.fonts.heading.family, "Source Serif 4", "headings in the serif");
        for id in FONTS_ONLY {
            let font = design::load_font(&repo(Part::Font), id).expect("valid");
            assert!(font.kind.is_some() && font.license == "OFL-1.1", "{id}: {:?}", font.kind);
        }
        // Console suggests JetBrains Mono (`[about] font`); the id must stay what the design names.
        let console = design::load(&repo(Part::Design), "console").expect("built in");
        assert_eq!(console.about.font.as_deref(), Some("jetbrains-mono"));
        let mono = design::load_font(&repo(Part::Font), "jetbrains-mono").expect("valid");
        assert_eq!(mono.kind, Some(torro_core::design::FontKind::Mono));
    }

    #[test]
    fn dracula_is_a_colour_scheme_only() {
        let scheme = design::load_color_scheme(&repo(Part::ColorScheme), "dracula").expect("valid");
        let rgb = Rgba::hex;
        assert_eq!((scheme.colors.dark.panel, scheme.colors.light.panel), (rgb(0x282A36), rgb(0xFFFBEB)));
        assert_eq!((scheme.colors.dark.accent, scheme.colors.dark.focus_ring), (rgb(0xBD93F9), rgb(0xFF79C6)));
        assert!(
            matches!(design::load(&repo(Part::Design), "dracula"), Err(p) if matches!(p[..], [DesignError::NotFound { .. }]))
        );
        assert_ne!(scheme, ColorScheme::standard());
    }

    #[test]
    fn a_new_data_folder_gets_every_example_and_the_lists() {
        let home = tempfile::tempdir().expect("temp dir");
        let all = folders(&home.path().join("data"));
        let errors = install_all(&all);
        assert!(errors.is_empty(), "{errors:?}");
        for part in PARTS {
            let mut expected = ids(part);
            expected.push(OFFERED.to_string());
            expected.sort();
            assert_eq!(entries(all.get(part)), expected, "{part:?}");
            assert_eq!(offered(all.get(part)), ids(part));
        }
        assert!(design::list(&all.designs).iter().all(|entry| entry.valid()), "the copies are valid, the list is none");
        assert!(design::list_fonts(&all.fonts).iter().all(|entry| entry.valid()));
        assert!(design::list_color_schemes(&all.color_schemes).iter().all(|entry| entry.valid()));
        let before = everything(&all);
        install_all(&all);
        assert_eq!(everything(&all), before, "nothing twice");
        let left = entries(&home.path().join("data"));
        assert_eq!(left, ["color-schemes", "designs", "fonts"], "no staging folder stays behind");
    }

    /// An old designs folder keeps what it has – its Plex too, which is no example any more (issue
    /// #291: copies of the old examples are left to their owner) – and gets its list.
    #[test]
    fn an_old_designs_folder_keeps_its_plex_and_gets_its_list() {
        let home = tempfile::tempdir().expect("temp dir");
        let dir = home.path().join("designs");
        fs::create_dir_all(dir.join("plex")).expect("plex");
        fs::write(dir.join("plex").join("design.toml"), "format = 1\n").expect("changed plex");
        let added = install_new(&dir, Part::Design).expect("install");
        assert_eq!(added, Vec::<&str>::new(), "there are no example designs to add");
        assert_eq!(fs::read_to_string(dir.join("plex").join("design.toml")).expect("read"), "format = 1\n");
        assert_eq!(entries(&dir), [OFFERED, "plex"], "no hidden work folder stays behind");
    }

    #[test]
    fn a_deleted_example_stays_deleted() {
        let home = tempfile::tempdir().expect("temp dir");
        // An old designs folder whose Plex the user deleted: Plex does not come back, the others do.
        let old = home.path().join("old");
        fs::create_dir(&old).expect("empty designs folder");
        assert!(!install_new(&old, Part::Design).expect("install").contains(&"plex"));
        assert!(!old.join("plex").exists());

        // Deleted after it was offered: it stays deleted, in every folder.
        let all = folders(&home.path().join("data"));
        install_all(&all);
        for part in [Part::Font, Part::ColorScheme] {
            let gone = examples(part)[examples(part).len() - 1].id;
            fs::remove_dir_all(all.get(part).join(gone)).expect("delete");
        }
        let before = everything(&all);
        install_all(&all);
        assert_eq!(everything(&all), before);
    }

    /// Task L.20: each folder keeps its own list; a new fonts or colour schemes folder gets all of
    /// its examples, whatever the designs folder was offered.
    #[test]
    fn each_folder_keeps_its_own_list() {
        let home = tempfile::tempdir().expect("temp dir");
        let all = folders(&home.path().join("data"));
        fs::create_dir_all(&all.designs).expect("designs");
        fs::write(all.designs.join(OFFERED), list_text(&ids(Part::Design))).expect("every design offered");
        let errors = install_all(&all);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(entries(&all.designs), [OFFERED], "deleted designs stay deleted");
        for part in [Part::Font, Part::ColorScheme] {
            assert_eq!(offered(all.get(part)), ids(part));
            assert!(ids(part).iter().all(|id| all.get(part).join(id).is_dir()), "{part:?}");
        }
        // A colour schemes folder the user made without a list gets every example it has no folder of.
        let own = folders(&home.path().join("own"));
        fs::create_dir_all(own.color_schemes.join("nord")).expect("own nord");
        install_new(&own.color_schemes, Part::ColorScheme).expect("install");
        assert_eq!(entries(&own.color_schemes.join("nord")), Vec::<String>::new(), "theirs stays theirs");
        assert!(own.color_schemes.join("dracula").is_dir());
    }

    #[test]
    fn a_folder_of_the_users_own_with_an_examples_name_stays_theirs() {
        let home = tempfile::tempdir().expect("temp dir");
        for part in [Part::Font, Part::ColorScheme] {
            let dir = home.path().join(format!("{part:?}"));
            let own = examples(part)[examples(part).len() - 1].id;
            fs::create_dir_all(dir.join(own)).expect("own");
            fs::write(dir.join(OFFERED), "plex\n").expect("an older list");
            fs::write(dir.join(own).join("notes.txt"), "mine").expect("own file");
            assert!(!install_new(&dir, part).expect("install").contains(&own));
            assert_eq!(entries(&dir.join(own)), ["notes.txt"]);
            assert!(offered(&dir).iter().any(|id| id == own), "offered, so it is not tried again");
        }
    }

    #[test]
    fn a_changed_example_keeps_the_change() {
        let home = tempfile::tempdir().expect("temp dir");
        let all = folders(home.path());
        install_all(&all);
        let toml = all.fonts.join("plex").join("font.toml");
        fs::write(&toml, "format = 1\n").expect("change");
        fs::remove_file(all.fonts.join(OFFERED)).expect("even without its list");
        install_all(&all);
        assert_eq!(fs::read_to_string(&toml).expect("read"), "format = 1\n");
    }
}
