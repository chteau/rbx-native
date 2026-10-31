//! The game picker's "New experience": the form's state, its validation and
//! the two Roblox calls behind it (the groups to create in, the creation).

use super::*;

/// Answers both calls without the network: `ok` makes Create succeed
/// (universe 1, place 2), anything else is the refusal's text; either way the
/// groups are one mock group.
const MOCK_VARIABLE: &str = "RBX_STUDIO_NEW_EXPERIENCE_MOCK";

const NAME_MAX: usize = 50;
const DESCRIPTION_MAX: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldError {
    NameEmpty,
    NameLong,
    DescriptionLong,
}

impl FieldError {
    pub(super) fn message(self) -> &'static str {
        match self {
            FieldError::NameEmpty => "Enter a name.",
            FieldError::NameLong => "Names can be up to 50 characters.",
            FieldError::DescriptionLong => "Descriptions can be up to 1000 characters.",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct FieldErrors {
    pub(super) name: Option<FieldError>,
    pub(super) description: Option<FieldError>,
}

/// The trimmed name and description, or what is wrong with each. Lengths
/// count characters, as Roblox does.
pub(super) fn validate(name: &str, description: &str) -> Result<(String, String), FieldErrors> {
    let (name, description) = (name.trim(), description.trim());
    let errors = FieldErrors {
        name: match name.chars().count() {
            0 => Some(FieldError::NameEmpty),
            n if n > NAME_MAX => Some(FieldError::NameLong),
            _ => None,
        },
        description: (description.chars().count() > DESCRIPTION_MAX)
            .then_some(FieldError::DescriptionLong),
    };
    if errors == FieldErrors::default() {
        Ok((name.to_string(), description.to_string()))
    } else {
        Err(errors)
    }
}

/// The form's state beside its inputs.
pub(in crate::launcher) struct NewExperienceForm {
    pub(super) name: Entity<InputState>,
    pub(super) description: Entity<TextareaState>,
    pub(super) creator: Entity<SelectState<SearchableVec<SharedString>>>,
    /// The creator dropdown's rows, in order: `None` is "Me".
    pub(super) creators: Vec<(Option<u64>, SharedString)>,
    /// The rows changed; handed to the dropdown on the next render.
    pub(super) creators_dirty: bool,
    pub(super) chosen: Option<u64>,
    pub(super) loading_groups: bool,
    pub(super) groups_error: Option<String>,
    pub(super) errors: FieldErrors,
}

impl NewExperienceForm {
    pub(super) fn new(window: &mut Window, cx: &mut Context<HomeWindow>) -> Self {
        NewExperienceForm {
            name: cx.new(|cx| InputState::new(window, cx).placeholder("Untitled Experience")),
            description: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(3, 5)
                    .placeholder("Optional")
            }),
            creator: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(vec![SharedString::from("Me")]),
                    Some(IndexPath::new(0)),
                    window,
                    cx,
                )
            }),
            creators: vec![(None, "Me".into())],
            creators_dirty: false,
            chosen: None,
            loading_groups: false,
            groups_error: None,
            errors: FieldErrors::default(),
        }
    }
}

/// Keeps the form's chosen creator in step with its dropdown.
pub(super) fn follow_creator(
    form: &NewExperienceForm,
    cx: &mut Context<HomeWindow>,
) -> Subscription {
    cx.subscribe(&form.creator, |this, _, event: &SelectEvent<_>, cx| {
        let SelectEvent::Confirm(Some(label)) = event else {
            return;
        };
        let form = &mut this.new_experience;
        if let Some(&(id, _)) = form.creators.iter().find(|(_, l)| l == label) {
            form.chosen = id;
            cx.notify();
        }
    })
}

fn create_failure(err: &rbx_cloud::CloudError) -> String {
    match err {
        rbx_cloud::CloudError::Refused { status: 401, .. } => "Your Roblox sign-in has expired: sign in again in Settings \u{203a} Account \u{203a} Roblox sign-in.".to_string(),
        _ => err.to_string(),
    }
}

impl HomeWindow {
    pub(super) fn open_new_experience(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = &mut self.new_experience;
        form.name
            .update(cx, |input, cx| input.set_value("", window, cx));
        form.description
            .update(cx, |input, cx| input.set_value("", window, cx));
        form.creators = vec![(None, "Me".into())];
        form.creators_dirty = true;
        form.chosen = None;
        form.groups_error = None;
        form.errors = FieldErrors::default();
        self.dialog = Some(Dialog::NewExperience {
            busy: false,
            error: None,
        });
        self.load_creator_groups(cx);
        cx.notify();
    }

    /// Hands the dropdown its rows; a window to do it with is at hand in render.
    pub(super) fn sync_creator_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = &mut self.new_experience;
        if !std::mem::take(&mut form.creators_dirty) {
            return;
        }
        let labels: Vec<SharedString> = form.creators.iter().map(|(_, l)| l.clone()).collect();
        let selected = form
            .creators
            .iter()
            .position(|(id, _)| *id == form.chosen)
            .unwrap_or(0);
        form.creator.update(cx, |select, cx| {
            select.set_items(SearchableVec::new(labels), window, cx);
            select.set_selected_index(Some(IndexPath::new(selected)), window, cx);
        });
    }

    fn load_creator_groups(&mut self, cx: &mut Context<Self>) {
        if std::env::var(MOCK_VARIABLE).is_ok() {
            self.set_creator_groups(Ok(vec![rbx_cloud::CreatorGroup {
                id: 7,
                name: "Mock Studio Group".to_string(),
            }]));
            return;
        }
        let Some(session) = self.session.clone() else {
            return;
        };
        self.new_experience.loading_groups = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    rbx_cloud::creator_groups(&session).map_err(|err| create_failure(&err))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.set_creator_groups(result);
                cx.notify();
            });
        })
        .detach();
    }

    fn set_creator_groups(&mut self, result: Result<Vec<rbx_cloud::CreatorGroup>, String>) {
        let form = &mut self.new_experience;
        form.loading_groups = false;
        match result {
            Ok(groups) => {
                form.creators.extend(
                    groups
                        .into_iter()
                        .map(|group| (Some(group.id), group.name.into())),
                );
                form.creators_dirty = true;
            }
            Err(reason) => form.groups_error = Some(reason),
        }
    }

    pub(super) fn confirm_new_experience(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::NewExperience { busy, error }) = &mut self.dialog else {
            return;
        };
        let form = &mut self.new_experience;
        let name = form.name.read(cx).value().to_string();
        let description = form.description.read(cx).value().to_string();
        let (name, description) = match validate(&name, &description) {
            Ok(fields) => fields,
            Err(errors) => {
                form.errors = errors;
                cx.notify();
                return;
            }
        };
        form.errors = FieldErrors::default();
        let group = form.chosen;
        let mock = std::env::var(MOCK_VARIABLE).ok();
        let session = self.session.clone();
        *busy = true;
        *error = None;
        cx.notify();
        let sent = name.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    match (mock.as_deref(), session) {
                        (Some("ok"), _) => Ok((
                            rbx_cloud::CreatedExperience {
                                universe_id: 1,
                                root_place_id: 2,
                            },
                            None,
                        )),
                        (Some(reason), _) => Err(reason.to_string()),
                        (None, Some(cookie)) => {
                            let created = rbx_cloud::create_experience(&cookie, group)
                                .map_err(|err| create_failure(&err))?;
                            // Named apart from the create: the experience
                            // exists either way, and is linked either way.
                            let note = rbx_cloud::name_experience(
                                &cookie,
                                created,
                                &sent,
                                &description,
                            )
                            .err()
                            .map(|err| {
                                format!(
                                    "Roblox made experience {} but refused its name: {}. Rename it in Game Settings.",
                                    created.universe_id,
                                    create_failure(&err)
                                )
                            });
                            Ok((created, note))
                        }
                        (None, None) => Err("Not signed in to Roblox.".to_string()),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(Dialog::NewExperience { busy, error }) = &mut this.dialog else {
                    return;
                };
                match result {
                    Ok((created, note)) => {
                        this.dialog = None;
                        // Only the ids and the name are read downstream, so
                        // an unknown user id for "Me" is harmless.
                        let owner = match group {
                            Some(id) => rbx_cloud::Owner::Group(id),
                            None => rbx_cloud::Owner::User(this.user_id.unwrap_or(0)),
                        };
                        this.hand_over(
                            Experience {
                                universe_id: created.universe_id,
                                root_place_id: created.root_place_id,
                                name,
                                visibility: rbx_cloud::Visibility::Private,
                                owner,
                            },
                            note,
                            cx,
                        );
                    }
                    Err(reason) => {
                        *busy = false;
                        *error = Some(reason);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::{validate, FieldError};

    #[test]
    fn an_empty_or_blank_name_is_refused() {
        for name in ["", "   \t "] {
            assert_eq!(
                validate(name, "").unwrap_err().name,
                Some(FieldError::NameEmpty)
            );
        }
    }

    #[test]
    fn name_length_counts_characters_not_bytes() {
        assert!(validate(&"a".repeat(50), "").is_ok());
        assert!(validate(&"\u{e9}".repeat(50), "").is_ok());
        assert_eq!(
            validate(&"a".repeat(51), "").unwrap_err().name,
            Some(FieldError::NameLong)
        );
    }

    #[test]
    fn description_is_limited_to_1000_characters() {
        assert!(validate("n", &"\u{e9}".repeat(1000)).is_ok());
        assert_eq!(
            validate("n", &"a".repeat(1001)).unwrap_err().description,
            Some(FieldError::DescriptionLong)
        );
    }

    #[test]
    fn both_fields_are_reported_and_valid_input_is_trimmed() {
        let errors = validate("", &"a".repeat(1001)).unwrap_err();
        assert_eq!(errors.name, Some(FieldError::NameEmpty));
        assert_eq!(errors.description, Some(FieldError::DescriptionLong));
        assert_eq!(
            validate("  My Game ", " about \n").unwrap(),
            ("My Game".to_string(), "about".to_string())
        );
    }
}
