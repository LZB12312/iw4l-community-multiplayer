#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Main,
    Body,
    BodyShape,
    Merchandise,
    Accessories,
    Skateboards,
    Face,
    Eyes,
    Brow,
    Nose,
    Jaw,
    Mouth,
    Chin,
}

#[derive(Clone, Copy)]
pub enum Choice {
    Page(Page),
    Gender,
    Trucks,
    Wheels,
    Morph(&'static str),
    Undo(UndoScope),
    Unavailable,
}

#[derive(Clone, Copy)]
pub enum UndoScope {
    Appearance,
    Morphs,
}

#[derive(Clone, Copy)]
pub struct Item {
    pub label: &'static str,
    pub description: &'static str,
    pub kind: &'static str,
    pub choice: Choice,
    pub male_only: bool,
}

pub struct Menu {
    pub title: &'static str,
    pub kind: u8,
    pub info: &'static str,
    pub items: &'static [Item],
}

const fn morph(label: &'static str, target: &'static str) -> Item {
    Item {
        label,
        description: "",
        kind: "morph",
        choice: Choice::Morph(target),
        male_only: false,
    }
}

const UNDO: Item = Item {
    label: "ID_CAC_MORPHING_UNDO",
    description: "",
    kind: "option",
    choice: Choice::Undo(UndoScope::Appearance),
    male_only: false,
};

const UNDO_MORPHS: Item = Item {
    choice: Choice::Undo(UndoScope::Morphs),
    ..UNDO
};

fn morph_menu(title: &'static str, items: &'static [Item]) -> Menu {
    Menu {
        title,
        kind: 3,
        info: "petes",
        items,
    }
}

impl Page {
    pub fn focus(self) -> Option<Focus> {
        match self {
            Self::Main | Self::Body | Self::Merchandise => Some(Focus::Standing),
            Self::BodyShape => Some(Focus::Body),
            Self::Face => Some(Focus::Head),
            Self::Eyes => Some(Focus::Eyes),
            Self::Brow => Some(Focus::Brow),
            Self::Nose => Some(Focus::Nose),
            Self::Jaw => Some(Focus::Jaw),
            Self::Mouth => Some(Focus::Mouth),
            Self::Chin => Some(Focus::Chin),
            Self::Accessories | Self::Skateboards => None,
        }
    }

    pub fn menu(self) -> Menu {
        match self {
            Self::Main => Menu {
                title: "ID_CAC_EDITSKATER_TITLE",
                kind: 3,
                info: "blurb",
                items: &[
                    Item {
                        label: "ID_CAC_BACKALLEY_PETES_TITLE",
                        description: "ID_CAC_BACKALLEY_PETES_DESC",
                        kind: "option",
                        choice: Choice::Page(Page::Body),
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_SLAPPYS_MERCH_TITLE",
                        description: "ID_CAC_SLAPPYS_MERCH_DESC",
                        kind: "option",
                        choice: Choice::Page(Page::Merchandise),
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_SKATE_STYLE_TITLE",
                        description: "ID_CAC_SKATE_STYLE_DESC",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_LABEL_TRUCK_TIGHTNESS",
                        description: "ID_CAC_LABEL_TRUCK_DESC",
                        kind: "slider",
                        choice: Choice::Trucks,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_LABEL_WHEEL_STIFFNESS",
                        description: "ID_CAC_LABEL_WHEEL_DESC",
                        kind: "slider",
                        choice: Choice::Wheels,
                        male_only: false,
                    },
                ],
            },
            Self::Body => Menu {
                title: "ID_CAC_BODY_MODIFICATION_TITLE",
                kind: 0,
                info: "petes",
                items: &[
                    Item {
                        label: "ID_CAC_CHOOSE_GENDER_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Gender,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_SKIN_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_HAIR_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_BODY_SHAPE_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Page(Page::BodyShape),
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_PRESETS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_FACE_MODIFICATION_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Page(Page::Face),
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_FACIAL_HAIR_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: true,
                    },
                    Item {
                        label: "ID_CAC_UPPER_BODY_TATTOO",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_LOWER_BODY_TATTOO",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                ],
            },
            Self::Merchandise => Menu {
                title: "ID_CAC_MAINMENU_TITLE",
                kind: 0,
                info: "noblurb",
                items: &[
                    Item {
                        label: "ID_CAC_HATS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_TSHIRTS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_BUTTONSHIRTS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_HOODIES_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_JACKETS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_SWEATERS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_LEGS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_SHOES_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_SOCKS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_ACCESSORIES_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Page(Page::Accessories),
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_SKATEBOARDS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Page(Page::Skateboards),
                        male_only: false,
                    },
                ],
            },
            Self::Accessories => Menu {
                title: "ID_CAC_ACCESSORIES_TITLE",
                kind: 0,
                info: "noblurb",
                items: &[
                    Item {
                        label: "ID_CAC_GLASSES_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_WRIST_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_NECKLACES_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_RINGS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                ],
            },
            Self::Skateboards => Menu {
                title: "ID_CAC_SKATEBOARDS_TITLE",
                kind: 0,
                info: "noblurb",
                items: &[
                    Item {
                        label: "ID_CAC_BOARDS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_TRUCKS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_WHEELS_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Unavailable,
                        male_only: false,
                    },
                ],
            },
            Self::Face => Menu {
                title: "ID_CAC_FACE_MODIFICATION_TITLE",
                kind: 0,
                info: "petes",
                items: &[
                    Item {
                        label: "ID_CAC_EYES_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Page(Page::Eyes),
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_BROW_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Page(Page::Brow),
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_NOSE_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Page(Page::Nose),
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_JAW_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Page(Page::Jaw),
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_MOUTH_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Page(Page::Mouth),
                        male_only: false,
                    },
                    Item {
                        label: "ID_CAC_CHIN_TITLE",
                        description: "",
                        kind: "option",
                        choice: Choice::Page(Page::Chin),
                        male_only: false,
                    },
                ],
            },
            Self::BodyShape => morph_menu(
                "ID_CAC_BODY_SHAPE_TITLE",
                &const { [morph("ID_CAC_BODY_SHAPE_SIZE", "fat"), UNDO_MORPHS] },
            ),
            Self::Eyes => morph_menu(
                "ID_CAC_EYES_TITLE",
                &const {
                    [
                        Item {
                            label: "ID_CAC_EYES_COLOUR",
                            description: "",
                            kind: "option",
                            choice: Choice::Unavailable,
                            male_only: false,
                        },
                        morph("ID_CAC_EYES_ROTATION", "local_eye_rotation"),
                        morph("ID_CAC_EYES_WIDTH", "local_eye_width"),
                        morph("ID_CAC_EYES_HEIGHT", "local_eye_height"),
                        UNDO,
                    ]
                },
            ),
            Self::Brow => morph_menu(
                "ID_CAC_BROW_TITLE",
                &const {
                    [
                        Item {
                            label: "ID_CAC_BROW_STYLE",
                            description: "",
                            kind: "option",
                            choice: Choice::Unavailable,
                            male_only: false,
                        },
                        morph("ID_CAC_BROW_PROFILE", "local_brows_depth"),
                        morph("ID_CAC_BROW_ROTATION", "local_brows_rotation"),
                        morph("ID_CAC_BROW_HEIGHT", "local_brows_height"),
                        UNDO,
                    ]
                },
            ),
            Self::Nose => morph_menu(
                "ID_CAC_NOSE_TITLE",
                &const {
                    [
                        morph("ID_CAC_NOSE_LENGTH", "local_nose_length"),
                        morph("ID_CAC_NOSE_WIDTH", "local_nose_width"),
                        morph("ID_CAC_NOSE_HEIGHT", "local_nose_height"),
                        morph("ID_CAC_NOSE_CURVE", "local_nose_curve"),
                        UNDO,
                    ]
                },
            ),
            Self::Jaw => morph_menu(
                "ID_CAC_JAW_TITLE",
                &const {
                    [
                        morph("ID_CAC_JAW_DEFINITION", "local_jaw_chiseled"),
                        morph("ID_CAC_JAW_ROUNDNESS", "local_jaw_depth"),
                        UNDO,
                    ]
                },
            ),
            Self::Mouth => morph_menu(
                "ID_CAC_MOUTH_TITLE",
                &const {
                    [
                        morph("ID_CAC_MOUTH_WIDTH", "local_mouth_width"),
                        morph("ID_CAC_MOUTH_FULLNESS", "local_mouth_lipsize"),
                        morph("ID_CAC_MOUTH_SMILE", "local_mouth_corner"),
                        UNDO,
                    ]
                },
            ),
            Self::Chin => morph_menu(
                "ID_CAC_CHIN_TITLE",
                &const { [morph("ID_CAC_CHIN_LENGTH", "local_chin_length"), UNDO] },
            ),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Focus {
    #[default]
    Standing,
    Body,
    Head,
    Eyes,
    Brow,
    Nose,
    Jaw,
    Mouth,
    Chin,
}

impl Focus {
    pub fn key(self) -> &'static str {
        match self {
            Self::Standing => "standing",
            Self::Body => "body",
            Self::Head => "head",
            Self::Eyes => "eyes",
            Self::Brow => "brow",
            Self::Nose => "nose",
            Self::Jaw => "jaw",
            Self::Mouth => "mouth",
            Self::Chin => "chin",
        }
    }

    pub const ALL: [Self; 9] = [
        Self::Standing,
        Self::Body,
        Self::Head,
        Self::Eyes,
        Self::Brow,
        Self::Nose,
        Self::Jaw,
        Self::Mouth,
        Self::Chin,
    ];
}
