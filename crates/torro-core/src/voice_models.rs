//! The models the assistant speaks and listens with (issue #762, `docs/assistent.md` "Sprache"): a
//! fixed catalogue, so nothing but these files is ever downloaded. Each file names the Hugging Face
//! repository at a fixed revision, its SHA-256 and its size; the core downloads them
//! (`settings.download_voice_model`), the assistant app reads them from the voice folder.
//!
//! A model lives in `<voice>/models/<id>/` ([`Model::folder`]); it is ready once every file is
//! there with its size ([`Model::ready`]). The hash is checked while downloading, not on every
//! start: a gigabyte would take seconds to read.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::AppId;
use crate::paths::AppPaths;

/// What a model does ([`Model::kind`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VoiceModelKind {
    /// `"recognition"`: turns speech into text (dictating, "Hey Torro").
    Recognition,
    /// `"speech"`: reads answers aloud.
    Speech,
}

/// One file of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    /// Its name in the model's folder.
    pub name: &'static str,
    /// Where it is downloaded from, with the revision in the path.
    pub url: &'static str,
    /// Its SHA-256, lower-case hex.
    pub sha256: &'static str,
    /// Its size in bytes.
    pub bytes: u64,
}

/// A model of the catalogue ([`MODELS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    /// Its id in requests and folders: `whisper-turbo`, `parakeet`, `pocket-de`, `silero-vad`.
    pub id: &'static str,
    /// Its name for people; a product name, not translated.
    pub name: &'static str,
    pub kind: VoiceModelKind,
    /// Marked "(recommended)" in the settings.
    pub recommended: bool,
    pub files: &'static [ModelFile],
}

/// The model of Whisper turbo (decision 3 of #762): recognises German much better than Whisper
/// base, at 1 GB.
pub const WHISPER_TURBO: &str = "whisper-turbo";

/// The model of Parakeet TDT 0.6B v3, int8.
pub const PARAKEET: &str = "parakeet";

/// The German model of Pocket TTS with the voices of Kyutai (decision 1 of #762).
pub const POCKET_DE: &str = "pocket-de";

/// Silero VAD (issue #833): tells speech from noise frame by frame, so a sentence starts and ends with
/// speech instead of loudness. A download like the others, a recognition model, never recommended: without
/// it the listener keeps telling speech by loudness.
pub const SILERO_VAD: &str = "silero-vad";

/// Smart Turn v3 of Pipecat (issue #836): hears in the tone of the last seconds whether a person finished
/// speaking, German and English among its languages. A recognition download after Silero VAD, never
/// recommended, and only used with it: without it the listener judges the end of a sentence by its text. The
/// file without quantisation ("gpu", 32 MB, it runs on the processor too): tract does not load the quantised
/// one of 8.7 MB (its convolution in int8).
pub const SMART_TURN: &str = "smart-turn";

/// The wake word model "Hey Torro" (issue #765): built into the assistant, so it is not in the catalogue
/// [`MODELS`] and never downloaded; `settings.voice_status` names it for the wake word.
pub const HEY_TORRO: &str = "hey-torro";

/// Qwen3-TTS 0.6B Base for Crane (#764): the talker and its speech tokenizer as Qwen publishes them,
/// read without conversion. Crane wants the tokenizer in a folder `speech_tokenizer/`; the catalogue
/// keeps every file flat in the model's folder, so its two files carry the folder in their names and
/// the assistant links them into place when it loads the model.
pub const QWEN: &str = "qwen3-tts";

/// Every model, in the order the settings show them: speech recognition first, the recommended one on
/// top, Silero VAD and Smart Turn after the voices, Qwen3-TTS last and only in builds with it
/// (`crate::settings::QWEN_BUILT`).
#[cfg(feature = "qwen-tts")]
pub const MODELS: [Model; 6] = [BUILT_IN[0], BUILT_IN[1], BUILT_IN[2], BUILT_IN[3], BUILT_IN[4], QWEN_MODEL];

/// Every model, in the order the settings show them: speech recognition first, the recommended one on top,
/// Silero VAD and Smart Turn after the voices.
#[cfg(not(feature = "qwen-tts"))]
pub const MODELS: [Model; 5] = BUILT_IN;

/// The model of Qwen3-TTS ([`QWEN`]), at the revision measured in `docs/messung-qwen3-tts.md`.
#[cfg_attr(
    not(any(feature = "qwen-tts", test)),
    expect(dead_code, reason = "listed only in builds with Qwen3-TTS; tested in every build")
)]
const QWEN_MODEL: Model = Model {
    id: QWEN,
    name: "Qwen3-TTS 0.6B",
    kind: VoiceModelKind::Speech,
    recommended: false,
    files: &[
        ModelFile {
            name: "config.json",
            url: "https://huggingface.co/Qwen/Qwen3-TTS-12Hz-0.6B-Base/resolve/5d83992436eae1d760afd27aff78a71d676296fc/config.json",
            sha256: "2e714c787c8edb98b05432685cddb634add2de4d4e645f653d68251ef72ba011",
            bytes: 4_494,
        },
        ModelFile {
            name: "vocab.json",
            url: "https://huggingface.co/Qwen/Qwen3-TTS-12Hz-0.6B-Base/resolve/5d83992436eae1d760afd27aff78a71d676296fc/vocab.json",
            sha256: "ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910",
            bytes: 2_776_833,
        },
        ModelFile {
            name: "merges.txt",
            url: "https://huggingface.co/Qwen/Qwen3-TTS-12Hz-0.6B-Base/resolve/5d83992436eae1d760afd27aff78a71d676296fc/merges.txt",
            sha256: "599bab54075088774b1733fde865d5bd747cbcc7a547c5bc12610e874e26f5e3",
            bytes: 1_671_839,
        },
        ModelFile {
            name: "tokenizer_config.json",
            url: "https://huggingface.co/Qwen/Qwen3-TTS-12Hz-0.6B-Base/resolve/5d83992436eae1d760afd27aff78a71d676296fc/tokenizer_config.json",
            sha256: "dc3c31c3bdaedd5016382bb3cbe07323026775ad51f5a4fb564505992ae4a670",
            bytes: 7_344,
        },
        ModelFile {
            name: "model.safetensors",
            url: "https://huggingface.co/Qwen/Qwen3-TTS-12Hz-0.6B-Base/resolve/5d83992436eae1d760afd27aff78a71d676296fc/model.safetensors",
            sha256: "180b3b10eb1c9f1b4db7806d5475bae3071c0243c299d49926bab1da3b6946f6",
            bytes: 1_829_344_272,
        },
        ModelFile {
            name: "speech_tokenizer.config.json",
            url: "https://huggingface.co/Qwen/Qwen3-TTS-12Hz-0.6B-Base/resolve/5d83992436eae1d760afd27aff78a71d676296fc/speech_tokenizer/config.json",
            sha256: "ee65bb901c876664ab8707c487157aa1a6ee57c65969b28fb5ec9dc211e68167",
            bytes: 2_336,
        },
        ModelFile {
            name: "speech_tokenizer.model.safetensors",
            url: "https://huggingface.co/Qwen/Qwen3-TTS-12Hz-0.6B-Base/resolve/5d83992436eae1d760afd27aff78a71d676296fc/speech_tokenizer/model.safetensors",
            sha256: "836b7b357f5ea43e889936a3709af68dfe3751881acefe4ecf0dbd30ba571258",
            bytes: 682_293_092,
        },
    ],
};

/// The models every build has.
const BUILT_IN: [Model; 5] = [
    Model {
        id: WHISPER_TURBO,
        name: "Whisper turbo",
        kind: VoiceModelKind::Recognition,
        recommended: true,
        files: &[
            ModelFile {
                name: "turbo-encoder.int8.onnx",
                url: "https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/resolve/2ca6ff69fc878651b770880507669577ac41c2ff/turbo-encoder.int8.onnx",
                sha256: "b02dcdf54f348741e93fe732b67d933c8dcb6735655f710640143081db38878b",
                bytes: 674_716_297,
            },
            ModelFile {
                name: "turbo-decoder.int8.onnx",
                url: "https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/resolve/2ca6ff69fc878651b770880507669577ac41c2ff/turbo-decoder.int8.onnx",
                sha256: "20accd02388482eb3a46bd615631adfdc85e1eb2c7db9ea3f02a40ffe6b81547",
                bytes: 361_080_764,
            },
            ModelFile {
                name: "turbo-tokens.txt",
                url: "https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/resolve/2ca6ff69fc878651b770880507669577ac41c2ff/turbo-tokens.txt",
                sha256: "b34b360dbb493e781e479794586d661700670d65564001f23024971d1f2fa126",
                bytes: 816_730,
            },
        ],
    },
    Model {
        id: PARAKEET,
        name: "Parakeet",
        kind: VoiceModelKind::Recognition,
        recommended: false,
        files: &[
            ModelFile {
                name: "encoder.int8.onnx",
                url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/ed5d745623472f97cfc2e6c2916ee248798fc5a8/encoder.int8.onnx",
                sha256: "1afbe69c1ffdabee94755f42e4daf1d93549131a3da11356bda84fcef0f2c479",
                bytes: 652_282_294,
            },
            ModelFile {
                name: "decoder.int8.onnx",
                url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/ed5d745623472f97cfc2e6c2916ee248798fc5a8/decoder.int8.onnx",
                sha256: "0222a86147299d4dee90edf95b6d13827edb42ad8b7c9cf0b2582f030937c0b5",
                bytes: 11_845_274,
            },
            ModelFile {
                name: "joiner.int8.onnx",
                url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/ed5d745623472f97cfc2e6c2916ee248798fc5a8/joiner.int8.onnx",
                sha256: "cbd02f3696e9944970111fb439d9c3fd365a80374565f13f2b9c7007d253fc98",
                bytes: 6_355_277,
            },
            ModelFile {
                name: "tokens.txt",
                url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/ed5d745623472f97cfc2e6c2916ee248798fc5a8/tokens.txt",
                sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
                bytes: 93_939,
            },
        ],
    },
    Model {
        id: POCKET_DE,
        name: "Pocket TTS, German",
        kind: VoiceModelKind::Speech,
        recommended: true,
        files: &[
            ModelFile {
                name: "lm_flow.int8.onnx",
                url: "https://huggingface.co/haex-space/pocket-tts-sherpa-onnx/resolve/d8f5915ed8bd94e1aa3dbd5a219360b3bb966190/de/lm_flow.int8.onnx",
                sha256: "2b0eb8381c49672ac1050c50221db02033b5dc6c346ab45c67bd05344257f06c",
                bytes: 9_962_530,
            },
            ModelFile {
                name: "lm_main.int8.onnx",
                url: "https://huggingface.co/haex-space/pocket-tts-sherpa-onnx/resolve/d8f5915ed8bd94e1aa3dbd5a219360b3bb966190/de/lm_main.int8.onnx",
                sha256: "3d5e9cd303ecd26fc75b91e2766bce6de3d274d397d3ee9a392f6299366681ef",
                bytes: 76_341_079,
            },
            ModelFile {
                name: "encoder.onnx",
                url: "https://huggingface.co/haex-space/pocket-tts-sherpa-onnx/resolve/d8f5915ed8bd94e1aa3dbd5a219360b3bb966190/de/encoder.onnx",
                sha256: "c5a02b66a1720396da7edeb252aa3080603e4b079b87c5e2ffcb74c95aba2912",
                bytes: 20_783_791,
            },
            ModelFile {
                name: "decoder.int8.onnx",
                url: "https://huggingface.co/haex-space/pocket-tts-sherpa-onnx/resolve/d8f5915ed8bd94e1aa3dbd5a219360b3bb966190/de/decoder.int8.onnx",
                sha256: "3fe868810d66fa0c26bb3ae0d5ee65cae83028c0fe83398bcf3d96478f68083b",
                bytes: 22_684_077,
            },
            ModelFile {
                name: "text_conditioner.onnx",
                url: "https://huggingface.co/haex-space/pocket-tts-sherpa-onnx/resolve/d8f5915ed8bd94e1aa3dbd5a219360b3bb966190/de/text_conditioner.onnx",
                sha256: "de27b06fcd3901d50fdd95d669ba7a59e8311e4dba4120635eca3f3a898871b7",
                bytes: 16_388_384,
            },
            ModelFile {
                name: "vocab.json",
                url: "https://huggingface.co/haex-space/pocket-tts-sherpa-onnx/resolve/d8f5915ed8bd94e1aa3dbd5a219360b3bb966190/de/vocab.json",
                sha256: "c78ab36f541fdf5971472ca589a4619a3173522680b2fd58d34c5dd443f214fc",
                bytes: 69_903,
            },
            ModelFile {
                name: "token_scores.json",
                url: "https://huggingface.co/haex-space/pocket-tts-sherpa-onnx/resolve/d8f5915ed8bd94e1aa3dbd5a219360b3bb966190/de/token_scores.json",
                sha256: "cd446b01d12e68f951757bdc703fc96c31ecefa0b186b4adf569a74c571161a6",
                bytes: 124_036,
            },
            ModelFile {
                name: "alba.wav",
                url: "https://huggingface.co/kyutai/tts-voices/resolve/323332d33f997de8394f24a193e1a76df720e01a/alba-mackenna/casual.wav",
                sha256: "46264e83cb99115c3d210260e029117566d9c64f20266d10daa78107759ede3e",
                bytes: 958_542,
            },
            ModelFile {
                name: "fantine.wav",
                url: "https://huggingface.co/kyutai/tts-voices/resolve/323332d33f997de8394f24a193e1a76df720e01a/vctk/p244_023_enhanced.wav",
                sha256: "5f07d4e2a3f20a15572aae885156b43ef3fc12ef3812996fd135680d9956448b",
                bytes: 674_852,
            },
            ModelFile {
                name: "marius.wav",
                url: "https://huggingface.co/kyutai/tts-voices/resolve/323332d33f997de8394f24a193e1a76df720e01a/voice-donations/Selfie.wav",
                sha256: "076968c3122520f3412eb7090e8c1c3f75fe57be1e24a2f96465583d84c71e16",
                bytes: 480_044,
            },
        ],
    },
    Model {
        id: SILERO_VAD,
        name: "Silero VAD",
        kind: VoiceModelKind::Recognition,
        recommended: false,
        files: &[ModelFile {
            name: "silero_vad.onnx",
            url: "https://huggingface.co/csukuangfj/vad/resolve/fba88cd2e921609e7675c3aaf51e0b9b295da4bc/silero_vad.onnx",
            sha256: "a35ebf52fd3ce5f1469b2a36158dba761bc47b973ea3382b3186ca15b1f5af28",
            bytes: 1_807_522,
        }],
    },
    Model {
        id: SMART_TURN,
        name: "Smart Turn",
        kind: VoiceModelKind::Recognition,
        recommended: false,
        files: &[ModelFile {
            name: "smart-turn-v3.2-gpu.onnx",
            url: "https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/f766f81d3cfdf7737ac64aad813d91bbfd56bf93/smart-turn-v3.2-gpu.onnx",
            sha256: "ab8dc64b88713f90b571c15b714bd1330e6c883cad8763dacf65c9376dc539be",
            bytes: 32_411_198,
        }],
    },
];

/// The model `id`, if the catalogue has it.
pub fn model(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|model| model.id == id)
}

/// The folder where the assistant keeps its voice files and models (`<data>/torroassistant/voice`);
/// `None` without a home directory.
pub fn voice_folder() -> Option<PathBuf> {
    AppPaths::for_app(AppId::ASSISTANT).map(|paths| paths.data.join("voice"))
}

impl Model {
    /// The bytes of all its files.
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|file| file.bytes).sum()
    }

    /// Its folder in the voice folder `voice`: `<voice>/models/<id>`.
    pub fn folder(&self, voice: &Path) -> PathBuf {
        voice.join("models").join(self.id)
    }

    /// The file `name` in its folder.
    pub fn path(&self, voice: &Path, name: &str) -> PathBuf {
        self.folder(voice).join(name)
    }

    /// Whether every file is there with its size: then the model was downloaded and checked.
    pub fn ready(&self, voice: &Path) -> bool {
        self.files.iter().all(|file| {
            std::fs::metadata(self.path(voice, file.name)).is_ok_and(|meta| meta.is_file() && meta.len() == file.bytes)
        })
    }
}

/// The voice files of Pocket TTS in [`POCKET_DE`], by the voice id of the setting `pocket_voice`
/// (`crate::settings::POCKET_VOICES`). The templates `warm_woman` and `calm_man` ship with the app.
pub const POCKET_VOICE_FILES: [(&str, &str); 3] =
    [("alba", "alba.wav"), ("fantine", "fantine.wav"), ("marius", "marius.wav")];

/// The languages Whisper recognises, as its code and the name in the language itself ("Deutsch",
/// "English"), so the translation table does not need a hundred names. Sorted by the code.
pub const WHISPER_LANGUAGES: [(&str, &str); 100] = [
    ("af", "Afrikaans"),
    ("am", "አማርኛ"),
    ("ar", "العربية"),
    ("as", "অসমীয়া"),
    ("az", "Azərbaycanca"),
    ("ba", "Башҡортса"),
    ("be", "Беларуская"),
    ("bg", "Български"),
    ("bn", "বাংলা"),
    ("bo", "བོད་སྐད་"),
    ("br", "Brezhoneg"),
    ("bs", "Bosanski"),
    ("ca", "Català"),
    ("cs", "Čeština"),
    ("cy", "Cymraeg"),
    ("da", "Dansk"),
    ("de", "Deutsch"),
    ("el", "Ελληνικά"),
    ("en", "English"),
    ("es", "Español"),
    ("et", "Eesti"),
    ("eu", "Euskara"),
    ("fa", "فارسی"),
    ("fi", "Suomi"),
    ("fo", "Føroyskt"),
    ("fr", "Français"),
    ("gl", "Galego"),
    ("gu", "ગુજરાતી"),
    ("ha", "Hausa"),
    ("haw", "ʻŌlelo Hawaiʻi"),
    ("he", "עברית"),
    ("hi", "हिन्दी"),
    ("hr", "Hrvatski"),
    ("ht", "Kreyòl ayisyen"),
    ("hu", "Magyar"),
    ("hy", "Հայերեն"),
    ("id", "Bahasa Indonesia"),
    ("is", "Íslenska"),
    ("it", "Italiano"),
    ("ja", "日本語"),
    ("jw", "Basa Jawa"),
    ("ka", "ქართული"),
    ("kk", "Қазақша"),
    ("km", "ខ្មែរ"),
    ("kn", "ಕನ್ನಡ"),
    ("ko", "한국어"),
    ("la", "Latina"),
    ("lb", "Lëtzebuergesch"),
    ("ln", "Lingála"),
    ("lo", "ລາວ"),
    ("lt", "Lietuvių"),
    ("lv", "Latviešu"),
    ("mg", "Malagasy"),
    ("mi", "Māori"),
    ("mk", "Македонски"),
    ("ml", "മലയാളം"),
    ("mn", "Монгол"),
    ("mr", "मराठी"),
    ("ms", "Bahasa Melayu"),
    ("mt", "Malti"),
    ("my", "မြန်မာ"),
    ("ne", "नेपाली"),
    ("nl", "Nederlands"),
    ("nn", "Nynorsk"),
    ("no", "Norsk"),
    ("oc", "Occitan"),
    ("pa", "ਪੰਜਾਬੀ"),
    ("pl", "Polski"),
    ("ps", "پښتو"),
    ("pt", "Português"),
    ("ro", "Română"),
    ("ru", "Русский"),
    ("sa", "संस्कृतम्"),
    ("sd", "سنڌي"),
    ("si", "සිංහල"),
    ("sk", "Slovenčina"),
    ("sl", "Slovenščina"),
    ("sn", "ChiShona"),
    ("so", "Soomaali"),
    ("sq", "Shqip"),
    ("sr", "Српски"),
    ("su", "Basa Sunda"),
    ("sv", "Svenska"),
    ("sw", "Kiswahili"),
    ("ta", "தமிழ்"),
    ("te", "తెలుగు"),
    ("tg", "Тоҷикӣ"),
    ("th", "ไทย"),
    ("tk", "Türkmençe"),
    ("tl", "Tagalog"),
    ("tr", "Türkçe"),
    ("tt", "Татарча"),
    ("uk", "Українська"),
    ("ur", "اردو"),
    ("uz", "Oʻzbekcha"),
    ("vi", "Tiếng Việt"),
    ("yi", "ייִדיש"),
    ("yo", "Yorùbá"),
    ("yue", "粵語"),
    ("zh", "中文"),
];

/// The name of the Whisper language `code` in that language; `None` for a code Whisper does not know.
pub fn whisper_language(code: &str) -> Option<&'static str> {
    WHISPER_LANGUAGES.iter().find(|(known, _)| *known == code).map(|(_, name)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_names_each_file_once_with_a_fixed_revision_and_hash() {
        for model in MODELS {
            assert!(model.bytes() > 0, "{}", model.id);
            for file in model.files {
                assert!(file.url.starts_with("https://huggingface.co/"), "{}", file.url);
                assert!(file.url.contains("/resolve/"), "a fixed revision: {}", file.url);
                assert!(!file.url.contains("/main/"), "never a moving branch: {}", file.url);
                assert_eq!(file.sha256.len(), 64, "{}", file.name);
                assert!(file.sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
                assert!(!file.name.contains('/') && !file.name.contains(".."), "{}", file.name);
                assert_eq!(model.files.iter().filter(|other| other.name == file.name).count(), 1);
            }
        }
        assert_eq!(model("parakeet").map(|model| model.kind), Some(VoiceModelKind::Recognition));
        assert!(model("../etc").is_none());
        let recommended = |kind| MODELS.iter().filter(|model| model.kind == kind && model.recommended).count();
        assert_eq!((recommended(VoiceModelKind::Recognition), recommended(VoiceModelKind::Speech)), (1, 1));
        let pocket = model(POCKET_DE).expect("pocket");
        for (_, file) in POCKET_VOICE_FILES {
            assert!(pocket.files.iter().any(|known| known.name == file), "{file}");
        }
    }

    /// Issue #764: Qwen3-TTS is in the catalogue only where it is built, after Pocket TTS and not
    /// recommended; its files come from one revision, each named after its path in the repository.
    #[test]
    fn qwen_is_listed_only_where_it_is_built() {
        assert_eq!(model(QWEN).is_some(), crate::settings::QWEN_BUILT);
        assert_eq!(MODELS.len(), 5 + usize::from(crate::settings::QWEN_BUILT));
        assert_eq!(QWEN_MODEL.kind, VoiceModelKind::Speech);
        const { assert!(!QWEN_MODEL.recommended, "Pocket TTS stays the recommended voice") };
        let revision = "/resolve/5d83992436eae1d760afd27aff78a71d676296fc/";
        assert!(QWEN_MODEL.files.iter().all(|file| file.url.contains(revision)));
        for file in QWEN_MODEL.files {
            let path = file.url.split(revision).nth(1).unwrap_or_default();
            assert_eq!(path.replace('/', "."), file.name, "the name is its path with the folder in it");
        }
        assert!(QWEN_MODEL.bytes() > 2_000_000_000, "about 2.5 GB");
    }

    /// Issue #833: Silero VAD is a small recognition download after the voices, never recommended.
    #[test]
    fn silero_vad_is_a_small_download_after_the_voices() {
        let vad = model(SILERO_VAD).expect("silero vad");
        assert_eq!((vad.name, vad.kind, vad.recommended), ("Silero VAD", VoiceModelKind::Recognition, false));
        assert_eq!(vad.files.iter().map(|file| file.name).collect::<Vec<_>>(), ["silero_vad.onnx"]);
        assert_eq!(vad.bytes(), 1_807_522, "1.8 MB");
        assert_eq!(MODELS.iter().position(|model| model.id == SILERO_VAD), Some(3), "after Pocket TTS");
    }

    /// Issue #836: Smart Turn is a recognition download right after Silero VAD, never recommended.
    #[test]
    fn smart_turn_is_a_download_after_silero_vad() {
        let turn = model(SMART_TURN).expect("smart turn");
        assert_eq!((turn.name, turn.kind, turn.recommended), ("Smart Turn", VoiceModelKind::Recognition, false));
        assert_eq!(turn.files.iter().map(|file| file.name).collect::<Vec<_>>(), ["smart-turn-v3.2-gpu.onnx"]);
        assert_eq!(turn.bytes(), 32_411_198, "32 MB");
        assert_eq!(MODELS.iter().position(|model| model.id == SMART_TURN), Some(4), "after Silero VAD");
    }

    #[test]
    fn a_model_is_ready_once_every_file_has_its_size() {
        let dir = tempfile::tempdir().expect("temp dir");
        let parakeet = model(PARAKEET).expect("parakeet");
        assert!(!parakeet.ready(dir.path()));
        std::fs::create_dir_all(parakeet.folder(dir.path())).expect("folder");
        for file in parakeet.files {
            let file_ = std::fs::File::create(parakeet.path(dir.path(), file.name)).expect("file");
            file_.set_len(file.bytes).expect("size");
        }
        assert!(parakeet.ready(dir.path()));
        let last = parakeet.files[parakeet.files.len() - 1];
        std::fs::File::create(parakeet.path(dir.path(), last.name)).expect("cut").set_len(1).expect("size");
        assert!(!parakeet.ready(dir.path()), "a cut file is not ready");
        assert_eq!(parakeet.folder(dir.path()), dir.path().join("models").join("parakeet"));
    }

    #[test]
    fn whisper_languages_are_unique_and_named_in_their_own_language() {
        let mut codes: Vec<&str> = WHISPER_LANGUAGES.iter().map(|(code, _)| *code).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), WHISPER_LANGUAGES.len());
        assert_eq!((whisper_language("de"), whisper_language("en")), (Some("Deutsch"), Some("English")));
        assert_eq!(whisper_language("auto"), None);
    }
}
