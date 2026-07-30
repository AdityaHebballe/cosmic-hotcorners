// SPDX-License-Identifier: MPL-2.0

use crate::config::{Config, CornerAction};
use cosmic::ApplicationExt;
use cosmic::Element;
use cosmic::app::Task;
use cosmic::cctk::sctk::reexports::client::protocol::wl_output::WlOutput;
use cosmic::core::AppType;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::event::listen_with;
use cosmic::iced::event::wayland::{Event as WaylandEvent, OutputEvent};
use cosmic::iced::futures::{SinkExt, Stream, StreamExt, stream as futures_stream};
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{
    IcedOutput, SctkLayerSurfaceSettings,
};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, KeyboardInteractivity, Layer, destroy_layer_surface,
};
use cosmic::iced::runtime::platform_specific::wayland::CornerRadius;
use cosmic::iced::{Border, Event, Length, Subscription, mouse, stream, window};
use cosmic::surface::action::{LiveSettings, simple_layer_shell};
use cosmic::widget;
use std::time::Duration;

const CORNER_SIZE: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    fn anchor(self) -> Anchor {
        match self {
            Corner::TopLeft => Anchor::TOP.union(Anchor::LEFT),
            Corner::TopRight => Anchor::TOP.union(Anchor::RIGHT),
            Corner::BottomLeft => Anchor::BOTTOM.union(Anchor::LEFT),
            Corner::BottomRight => Anchor::BOTTOM.union(Anchor::RIGHT),
        }
    }
}

const CORNERS: [Corner; 4] = [
    Corner::TopLeft,
    Corner::TopRight,
    Corner::BottomLeft,
    Corner::BottomRight,
];

pub struct AppModel {
    core: cosmic::Core,
    outputs: Vec<(WlOutput, [(window::Id, Corner); 4])>,
    config: Config,
    pending_generation: u64,
    active_corner: Option<window::Id>,
    workspaces_visible: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    CursorMoved(window::Id),
    CursorLeft(window::Id),
    TriggerCorner(Corner, u64),
    WorkspacesVisibilityChanged(bool),
    OutputAdded(WlOutput),
    OutputRemoved(WlOutput),
    ConfigUpdated(Config),
    Noop,
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "io.github.cosmic-hot-corners";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(mut core: cosmic::Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let config = cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
            .map(|ctx| match Config::get_entry(&ctx) {
                Ok(c) => c,
                Err((_, c)) => c,
            })
            .unwrap_or_default();

        core.set_app_type(AppType::System);

        (
            AppModel {
                core,
                outputs: Vec::new(),
                config,
                pending_generation: 0,
                active_corner: None,
                workspaces_visible: false,
            },
            Task::none(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        widget::Space::new().into()
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Message> {
        widget::container(
            widget::Space::new()
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .class(cosmic::theme::Container::custom(|_| {
            widget::container::Style {
                text_color: None,
                icon_color: None,
                background: None,
                border: Border::default(),
                shadow: Default::default(),
                snap: true,
            }
        }))
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let pointer_events = listen_with(|event, _status, window_id| match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => Some(Message::CursorMoved(window_id)),
            Event::Mouse(mouse::Event::CursorLeft) => Some(Message::CursorLeft(window_id)),
            Event::PlatformSpecific(cosmic::iced::event::PlatformSpecific::Wayland(
                WaylandEvent::Output(evt, output),
            )) => match evt {
                OutputEvent::Created(_) => Some(Message::OutputAdded(output)),
                OutputEvent::Removed => Some(Message::OutputRemoved(output)),
                _ => None,
            },
            _ => None,
        });

        let config_watch = self
            .watch_config::<Config>(Self::APP_ID)
            .map(|update| Message::ConfigUpdated(update.config));

        Subscription::batch([
            pointer_events,
            config_watch,
            Subscription::run_with("hot-corners-workspaces", |_| {
                workspace_visibility_subscription()
            }),
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OutputAdded(output) => {
                if self.outputs.iter().any(|(existing, _)| *existing == output) {
                    return Task::none();
                }
                let surfaces: [(window::Id, SctkLayerSurfaceSettings); 4] = CORNERS.map(|corner| {
                    let id = window::Id::unique();
                    let settings = SctkLayerSurfaceSettings {
                        id,
                        layer: Layer::Overlay,
                        keyboard_interactivity: KeyboardInteractivity::None,
                        input_zone: None,
                        anchor: corner.anchor(),
                        size: Some((Some(CORNER_SIZE), Some(CORNER_SIZE))),
                        exclusive_zone: -1,
                        output: IcedOutput::Output(output.clone()),
                        namespace: String::from("hot-corners"),
                        ..Default::default()
                    };
                    (id, settings)
                });

                let corner_ids: [(window::Id, Corner); 4] =
                    std::array::from_fn(|i| (surfaces[i].0, CORNERS[i]));

                self.outputs.push((output, corner_ids));

                let tasks: Vec<Task<Message>> = surfaces
                    .into_iter()
                    .map(|(_, settings)| {
                        cosmic::surface::surface_task(simple_layer_shell(
                            || LiveSettings {
                                // These 10px input-only surfaces must not inherit the
                                // normal themed radius, which may be larger than them.
                                corners: Some(CornerRadius::default()),
                                blur: Some(false),
                                ..Default::default()
                            },
                            move || settings.clone(),
                            None::<fn() -> Element<'static, cosmic::Action<Message>>>,
                        ))
                    })
                    .collect();
                return Task::batch(tasks);
            }
            Message::OutputRemoved(output) => {
                if let Some(pos) = self.outputs.iter().position(|(o, _)| *o == output) {
                    let (_, corner_ids) = self.outputs.remove(pos);
                    let tasks: Vec<Task<Message>> = corner_ids
                        .iter()
                        .map(|(id, _): &(window::Id, Corner)| destroy_layer_surface(*id))
                        .collect();
                    return Task::batch(tasks);
                }
            }
            Message::ConfigUpdated(config) => {
                self.config = config;
                // A changed configuration must not allow an already scheduled
                // activation to fire with stale settings.
                self.active_corner = None;
                self.pending_generation += 1;
            }
            Message::CursorMoved(id) => {
                let Some(corner) = self
                    .outputs
                    .iter()
                    .flat_map(|(_, ids): &(WlOutput, [(window::Id, Corner); 4])| ids.iter())
                    .find(|(cid, _)| *cid == id)
                    .map(|(_, c)| *c)
                else {
                    if self.active_corner.is_some() {
                        self.active_corner = None;
                        self.pending_generation += 1;
                    }
                    return Task::none();
                };

                if !self.config.enabled || matches!(self.action_for(corner), CornerAction::Disabled)
                {
                    if self.active_corner == Some(id) {
                        self.active_corner = None;
                        self.pending_generation += 1;
                    }
                    return Task::none();
                }
                if self.active_corner == Some(id) {
                    return Task::none();
                }
                self.active_corner = Some(id);
                self.pending_generation += 1;
                let trigger_gen = self.pending_generation;
                let delay = Duration::from_millis(self.config.delay_ms);
                return cosmic::task::future(async move {
                    tokio::time::sleep(delay).await;
                    Message::TriggerCorner(corner, trigger_gen)
                });
            }
            Message::CursorLeft(id) => {
                let known = self
                    .outputs
                    .iter()
                    .flat_map(|(_, ids): &(WlOutput, [(window::Id, Corner); 4])| ids.iter())
                    .any(|(cid, _)| *cid == id);
                if known {
                    self.active_corner = None;
                    self.pending_generation += 1;
                }
            }
            Message::TriggerCorner(corner, trigger_gen) => {
                if trigger_gen == self.pending_generation && self.config.enabled {
                    if matches!(self.action_for(corner), CornerAction::ShowWorkspaces) {
                        return set_workspaces_visible(!self.workspaces_visible);
                    }
                    return execute_action(self.action_for(corner));
                }
            }
            Message::WorkspacesVisibilityChanged(visible) => {
                self.workspaces_visible = visible;
            }
            Message::Noop => {}
        }
        Task::none()
    }
}

#[zbus::proxy(interface = "com.system76.CosmicWorkspaces")]
trait CosmicWorkspaces {
    #[zbus(signal)]
    async fn shown(&self);

    #[zbus(signal)]
    async fn hidden(&self);
}

fn workspace_visibility_subscription() -> impl Stream<Item = Message> {
    stream::channel(
        8,
        |mut output: cosmic::iced::futures::channel::mpsc::Sender<Message>| async move {
            let connection = match zbus::Connection::session().await {
                Ok(connection) => connection,
                Err(err) => {
                    eprintln!("[hot-corners] could not connect to the session bus: {err}");
                    return;
                }
            };
            let proxy = match CosmicWorkspacesProxy::new(
                &connection,
                "com.system76.CosmicWorkspaces",
                "/com/system76/CosmicWorkspaces",
            )
            .await
            {
                Ok(proxy) => proxy,
                Err(err) => {
                    eprintln!("[hot-corners] could not watch workspace visibility: {err}");
                    return;
                }
            };
            let shown = match proxy.receive_shown().await {
                Ok(stream) => stream.map(|_| true),
                Err(err) => {
                    eprintln!("[hot-corners] could not watch workspace-open events: {err}");
                    return;
                }
            };
            let hidden = match proxy.receive_hidden().await {
                Ok(stream) => stream.map(|_| false),
                Err(err) => {
                    eprintln!("[hot-corners] could not watch workspace-close events: {err}");
                    return;
                }
            };
            let mut events = futures_stream::select(shown, hidden);
            while let Some(visible) = events.next().await {
                if output
                    .send(Message::WorkspacesVisibilityChanged(visible))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        },
    )
}

impl AppModel {
    fn action_for(&self, corner: Corner) -> &CornerAction {
        match corner {
            Corner::TopLeft => &self.config.top_left,
            Corner::TopRight => &self.config.top_right,
            Corner::BottomLeft => &self.config.bottom_left,
            Corner::BottomRight => &self.config.bottom_right,
        }
    }
}

fn execute_action(action: &CornerAction) -> Task<Message> {
    match action {
        CornerAction::Disabled => Task::none(),
        CornerAction::ShowWorkspaces => set_workspaces_visible(true),
        CornerAction::OpenLauncher => cosmic::task::future(async {
            if let Err(err) = dbus_open_launcher().await {
                eprintln!("[hot-corners] could not open launcher: {err}");
            }
            Message::Noop
        }),
        CornerAction::RunCommand(cmd) => {
            if let Err(err) = std::process::Command::new("sh").args(["-c", cmd]).spawn() {
                eprintln!("[hot-corners] could not run command: {err}");
            }
            Task::none()
        }
    }
}

fn set_workspaces_visible(visible: bool) -> Task<Message> {
    cosmic::task::future(async move {
        let result = if visible {
            dbus_show_workspaces().await
        } else {
            dbus_hide_workspaces().await
        };
        match result {
            Ok(()) => Message::WorkspacesVisibilityChanged(visible),
            Err(err) => {
                let action = if visible { "show" } else { "hide" };
                eprintln!("[hot-corners] could not {action} workspaces: {err}");
                Message::Noop
            }
        }
    })
}

async fn dbus_show_workspaces() -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    conn.call_method(
        Some("com.system76.CosmicWorkspaces"),
        "/com/system76/CosmicWorkspaces",
        Some("com.system76.CosmicWorkspaces"),
        "Show",
        &(),
    )
    .await?;
    Ok(())
}

async fn dbus_hide_workspaces() -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    conn.call_method(
        Some("com.system76.CosmicWorkspaces"),
        "/com/system76/CosmicWorkspaces",
        Some("com.system76.CosmicWorkspaces"),
        "Hide",
        &(),
    )
    .await?;
    Ok(())
}

async fn dbus_open_launcher() -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    let args: std::collections::HashMap<&str, zbus::zvariant::Value<'_>> =
        std::collections::HashMap::new();
    conn.call_method(
        Some("com.system76.CosmicLauncher"),
        "/com/system76/CosmicLauncher",
        Some("org.freedesktop.DbusActivation"),
        "Activate",
        &args,
    )
    .await?;
    Ok(())
}
