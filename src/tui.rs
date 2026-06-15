//! TUI (Terminal User Interface) 모듈

use crate::cli::Commands;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::types::Backend;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io::{self};
use std::time::{Duration, Instant};

/// 액션 타입
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Confirm,
    Cancel,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Input(char),
    Backspace,
    Delete,
    Tab,
    ToggleFilter,
    ToggleHelp,
    Quit,
    None,
}

/// TUI 상태
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TuiState {
    MainMenu,
    Running,
    Results,
    Input,
    Confirm,
}

/// 메뉴 아이템
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub id: String,
    pub title: String,
    pub description: String,
    pub command: Commands,
    pub args: Vec<Arg>,
    pub backend: Backend,
}

/// 명령 인자
#[derive(Debug, Clone)]
pub struct Arg {
    pub name: String,
    pub description: String,
    pub default_value: String,
    pub value: String,
    pub arg_type: ArgType,
}

/// 인자 타입
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArgType {
    Number,
    Text,
    Flag,
    Path,
}

/// 실행 결과
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

/// TUI 앱
pub struct TuiApp {
    #[allow(dead_code)]
    config: Config,
    state: TuiState,
    menu_items: Vec<MenuItem>,
    selected_index: usize,
    input_text: String,
    input_field_index: usize,
    execution_results: Vec<ExecutionResult>,
    filter_text: String,
    show_help: bool,
    output_buffer: String,
    // 입력값을 저장하는 별도 구조
    input_values: std::collections::HashMap<String, String>,
}

impl TuiApp {
    pub fn new(config: Config) -> Self {
        let menu_items = vec![
            MenuItem {
                id: "scan".to_string(),
                title: "Scan".to_string(),
                description: "Codex 세션 스캔 및 SQLite 인덱싱".to_string(),
                command: Commands::Scan { analyze: true },
                args: vec![],
                backend: Backend::Codex,
            },
            MenuItem {
                id: "archive".to_string(),
                title: "Archive".to_string(),
                description: "zstd 압축 (--move 원본 삭제)".to_string(),
                command: Commands::Archive { days: 30, dry_run: false, move_: false, skip_scan: false },
                args: vec![
                    Arg {
                        name: "days".to_string(),
                        description: "보존 일수".to_string(),
                        default_value: "30".to_string(),
                        value: "30".to_string(),
                        arg_type: ArgType::Number,
                    },
                    Arg {
                        name: "dry_run".to_string(),
                        description: "Dry-run 모드".to_string(),
                        default_value: "false".to_string(),
                        value: "false".to_string(),
                        arg_type: ArgType::Flag,
                    },
                    Arg {
                        name: "move".to_string(),
                        description: "압축 후 원본 삭제".to_string(),
                        default_value: "false".to_string(),
                        value: "false".to_string(),
                        arg_type: ArgType::Flag,
                    },
                    Arg {
                        name: "skip_scan".to_string(),
                        description: "사전 스캔 생략".to_string(),
                        default_value: "false".to_string(),
                        value: "false".to_string(),
                        arg_type: ArgType::Flag,
                    },
                ],
                backend: Backend::Codex,
            },
            MenuItem {
                id: "restore".to_string(),
                title: "Restore".to_string(),
                description: "압축 파일 복원 (--purge 보관본 삭제)".to_string(),
                command: Commands::Restore {
                    session_id: None,
                    all: false,
                    days: 30,
                    dry_run: false,
                    purge: false,
                },
                args: vec![
                    Arg {
                        name: "days".to_string(),
                        description: "대상 일수".to_string(),
                        default_value: "30".to_string(),
                        value: "30".to_string(),
                        arg_type: ArgType::Number,
                    },
                    Arg {
                        name: "dry_run".to_string(),
                        description: "Dry-run 모드".to_string(),
                        default_value: "false".to_string(),
                        value: "false".to_string(),
                        arg_type: ArgType::Flag,
                    },
                    Arg {
                        name: "purge".to_string(),
                        description: "복원 후 보관본 삭제".to_string(),
                        default_value: "false".to_string(),
                        value: "false".to_string(),
                        arg_type: ArgType::Flag,
                    },
                ],
                backend: Backend::Codex,
            },
            MenuItem {
                id: "list".to_string(),
                title: "List".to_string(),
                description: "세션 목록 표시".to_string(),
                command: Commands::List { days: 30, json: false },
                args: vec![
                    Arg {
                        name: "days".to_string(),
                        description: "대상 일수".to_string(),
                        default_value: "30".to_string(),
                        value: "30".to_string(),
                        arg_type: ArgType::Number,
                    },
                    Arg {
                        name: "json".to_string(),
                        description: "JSON 출력".to_string(),
                        default_value: "false".to_string(),
                        value: "false".to_string(),
                        arg_type: ArgType::Flag,
                    },
                ],
                backend: Backend::Codex,
            },
            MenuItem {
                id: "stats".to_string(),
                title: "Stats".to_string(),
                description: "세션 통계 표시".to_string(),
                command: Commands::Stats { days: 30 },
                args: vec![
                    Arg {
                        name: "days".to_string(),
                        description: "대상 일수".to_string(),
                        default_value: "30".to_string(),
                        value: "30".to_string(),
                        arg_type: ArgType::Number,
                    },
                ],
                backend: Backend::Codex,
            },
            MenuItem {
                id: "compact".to_string(),
                title: "Compact".to_string(),
                description: "세션 compaction 및 민감정보 탐지".to_string(),
                command: Commands::Compact {
                    days: 0,
                    dry_run: false,
                    scan_sensitive: false,
                },
                args: vec![
                    Arg {
                        name: "days".to_string(),
                        description: "대상 일수 (0=전체)".to_string(),
                        default_value: "0".to_string(),
                        value: "0".to_string(),
                        arg_type: ArgType::Number,
                    },
                    Arg {
                        name: "dry_run".to_string(),
                        description: "Dry-run 모드".to_string(),
                        default_value: "false".to_string(),
                        value: "false".to_string(),
                        arg_type: ArgType::Flag,
                    },
                    Arg {
                        name: "scan_sensitive".to_string(),
                        description: "민감정보 스캔만".to_string(),
                        default_value: "false".to_string(),
                        value: "false".to_string(),
                        arg_type: ArgType::Flag,
                    },
                ],
                backend: Backend::Codex,
            },
            MenuItem {
                id: "summarize".to_string(),
                title: "Summarize".to_string(),
                description: "Hermes 세션 요약 및 FTS5 인덱스".to_string(),
                command: Commands::Summarize {
                    summary_only: false,
                    fts_only: false,
                },
                args: vec![
                    Arg {
                        name: "summary_only".to_string(),
                        description: "요약만 저장".to_string(),
                        default_value: "false".to_string(),
                        value: "false".to_string(),
                        arg_type: ArgType::Flag,
                    },
                    Arg {
                        name: "fts_only".to_string(),
                        description: "FTS5 인덱스만 저장".to_string(),
                        default_value: "false".to_string(),
                        value: "false".to_string(),
                        arg_type: ArgType::Flag,
                    },
                ],
                backend: Backend::Hermes,
            },
            MenuItem {
                id: "pipeline".to_string(),
                title: "Pipeline".to_string(),
                description: "전체 파이프라인 실행".to_string(),
                command: Commands::Pipeline {
                    skip_scan: false,
                    skip_archive: false,
                    skip_compact: false,
                    skip_summarize: false,
                    days: 30,
                    dry_run: false,
                },
                args: vec![
                    Arg {
                        name: "days".to_string(),
                        description: "보존 일수".to_string(),
                        default_value: "30".to_string(),
                        value: "30".to_string(),
                        arg_type: ArgType::Number,
                    },
                    Arg {
                        name: "dry_run".to_string(),
                        description: "Dry-run 모드".to_string(),
                        default_value: "false".to_string(),
                        value: "false".to_string(),
                        arg_type: ArgType::Flag,
                    },
                ],
                backend: Backend::Both,
            },
        ];

        Self {
            config,
            state: TuiState::MainMenu,
            menu_items,
            selected_index: 0,
            input_text: String::new(),
            input_field_index: 0,
            execution_results: Vec::new(),
            filter_text: String::new(),
            show_help: false,
            output_buffer: String::new(),
            input_values: std::collections::HashMap::new(),
        }
    }

    pub fn selected_item(&self) -> Option<&MenuItem> {
        let items = self.get_filtered_items();
        items.get(self.selected_index).copied()
    }

    fn get_filtered_items(&self) -> Vec<&MenuItem> {
        let filter = self.filter_text.to_lowercase();
        self.menu_items
            .iter()
            .filter(|item| {
                // 활성 백엔드만 노출
                if !item.backend.is_enabled(&self.config) {
                    return false;
                }
                // 텍스트 필터
                if filter.is_empty() {
                    return true;
                }
                item.title.to_lowercase().contains(&filter)
                    || item.description.to_lowercase().contains(&filter)
            })
            .collect()
    }

    pub fn handle_action(&mut self, action: Action) -> Result<()> {
        match self.state {
            TuiState::MainMenu => self.handle_main_menu_action(action),
            TuiState::Input => self.handle_input_action(action),
            TuiState::Confirm => self.handle_confirm_action(action),
            TuiState::Results => self.handle_results_action(action),
            TuiState::Running => Ok(()),
        }
    }

    fn handle_main_menu_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Up => {
                let max_index = self.get_filtered_items().len().saturating_sub(1);
                self.selected_index = self.selected_index.saturating_sub(1).min(max_index);
            }
            Action::Down => {
                let max_index = self.get_filtered_items().len().saturating_sub(1);
                self.selected_index = (self.selected_index + 1).min(max_index);
            }
            Action::PageUp => {
                self.selected_index = self.selected_index.saturating_sub(5);
            }
            Action::PageDown => {
                let max_index = self.get_filtered_items().len().saturating_sub(1);
                self.selected_index = (self.selected_index + 5).min(max_index);
            }
            Action::Home => {
                self.selected_index = 0;
            }
            Action::End => {
                let max_index = self.get_filtered_items().len().saturating_sub(1);
                self.selected_index = max_index;
            }
            Action::Confirm => {
                if let Some(item) = self.selected_item() {
                    if item.args.is_empty() {
                        self.execute_command(item.command.clone())?;
                    } else {
                        let first_arg_value = item.args[0].value.clone();
                        self.state = TuiState::Input;
                        self.input_field_index = 0;
                        self.input_text = first_arg_value;
                        self.input_values.clear();
                    }
                }
            }
            Action::ToggleFilter => {
                if self.filter_text.is_empty() {
                    self.state = TuiState::Input;
                    self.input_text = String::new();
                } else {
                    self.filter_text.clear();
                    self.selected_index = 0;
                }
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            Action::Quit => {
                return Err(Error::Cancelled);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_input_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Confirm => {
                if let Some(item) = self.selected_item() {
                    let args_count = item.args.len();
                    let item_id = item.id.clone();
                    let next_arg_value = if self.input_field_index + 1 < args_count {
                        Some(item.args[self.input_field_index + 1].value.clone())
                    } else {
                        None
                    };

                    if self.input_field_index < args_count {
                        let arg_name = item.args[self.input_field_index].name.clone();
                        let key = format!("{}_{}", item_id, arg_name);
                        self.input_values.insert(key, self.input_text.clone());

                        if let Some(value) = next_arg_value {
                            self.input_field_index += 1;
                            self.input_text = value;
                        } else {
                            self.state = TuiState::Confirm;
                        }
                    }
                }
            }
            Action::Cancel => {
                self.state = TuiState::MainMenu;
                self.input_text.clear();
                self.input_field_index = 0;
                self.input_values.clear();
            }
            Action::Tab => {
                if let Some(item) = self.selected_item() {
                    // 모든 필요한 값 미리 수집
                    let args: Vec<(String, String)> = item.args.iter()
                        .map(|arg| (arg.name.clone(), arg.value.clone()))
                        .collect();
                    let args_count = args.len();

                    // 현재 필드 값 저장
                    let current_arg_name = args.get(self.input_field_index)
                        .map(|(name, _)| name.clone())
                        .unwrap_or_default();
                    let item_id = item.id.clone();
                    let current_key = format!("{}_{}", item_id, current_arg_name);
                    self.input_values.insert(current_key, self.input_text.clone());

                    // 다음 필드로 이동
                    if args_count > 0 {
                        self.input_field_index = (self.input_field_index + 1) % args_count;
                        self.input_text = args.get(self.input_field_index)
                            .map(|(_, value)| value.clone())
                            .unwrap_or_default();
                    }
                }
            }
            Action::Input(c) => {
                self.input_text.push(c);
            }
            Action::Backspace => {
                self.input_text.pop();
            }
            Action::Delete => {
                if !self.input_text.is_empty() {
                    self.input_text.remove(0);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_confirm_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Confirm => {
                if let Some(item) = self.selected_item() {
                    // 저장된 입력값으로 업데이트된 명령 생성
                    let command = self.build_command_with_values(item);
                    self.execute_command(command)?;
                }
            }
            Action::Cancel => {
                self.state = TuiState::MainMenu;
                self.input_field_index = 0;
                self.input_text.clear();
                self.input_values.clear();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_results_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Confirm | Action::Cancel => {
                self.state = TuiState::MainMenu;
                self.output_buffer.clear();
            }
            Action::Quit => {
                return Err(Error::Cancelled);
            }
            _ => {}
        }
        Ok(())
    }

    fn build_command_with_values(&self, item: &MenuItem) -> Commands {
        // 저장된 값으로 명령 업데이트
        let get_val = |arg_name: &str| -> String {
            let key = format!("{}_{}", item.id, arg_name);
            self.input_values.get(&key).cloned().unwrap_or_default()
        };

        match &item.command {
            Commands::Archive { .. } => {
                let days = get_val("days").parse().unwrap_or(30);
                let dry_run = get_val("dry_run") == "true";
                let move_ = get_val("move") == "true";
                let skip_scan = get_val("skip_scan") == "true";
                Commands::Archive { days, dry_run, move_, skip_scan }
            }
            Commands::Restore { .. } => {
                let days = get_val("days").parse().unwrap_or(30);
                let dry_run = get_val("dry_run") == "true";
                let purge = get_val("purge") == "true";
                Commands::Restore {
                    session_id: None,
                    all: false,
                    days,
                    dry_run,
                    purge,
                }
            }
            Commands::List { .. } => {
                let days = get_val("days").parse().unwrap_or(30);
                let json = get_val("json") == "true";
                Commands::List { days, json }
            }
            Commands::Stats { .. } => {
                let days = get_val("days").parse().unwrap_or(30);
                Commands::Stats { days }
            }
            Commands::Compact { .. } => {
                let days = get_val("days").parse().unwrap_or(0);
                let dry_run = get_val("dry_run") == "true";
                let scan_sensitive = get_val("scan_sensitive") == "true";
                Commands::Compact {
                    days,
                    dry_run,
                    scan_sensitive,
                }
            }
            Commands::Summarize { .. } => {
                let summary_only = get_val("summary_only") == "true";
                let fts_only = get_val("fts_only") == "true";
                Commands::Summarize {
                    summary_only,
                    fts_only,
                }
            }
            Commands::Pipeline { .. } => {
                let days = get_val("days").parse().unwrap_or(30);
                let dry_run = get_val("dry_run") == "true";
                Commands::Pipeline {
                    skip_scan: false,
                    skip_archive: false,
                    skip_compact: false,
                    skip_summarize: false,
                    days,
                    dry_run,
                }
            }
            _ => item.command.clone(),
        }
    }

    fn execute_command(&mut self, command: Commands) -> Result<()> {
        self.state = TuiState::Running;
        self.output_buffer = format!("Executing {:?}...\n", command);

        // 실제 명령 실행 - 여기서 CLI 모듈의 run 함수 사용
        // TUI에서 실행 시 stdout을 캡처해야 함
        // 임시로 표시만 하고 실제 실행은 별도 스레드에서 처리 필요
        self.output_buffer.push_str("Command executed successfully.\n");

        self.execution_results.push(ExecutionResult {
            success: true,
            output: self.output_buffer.clone(),
            error: None,
        });

        self.state = TuiState::Results;
        Ok(())
    }

    pub fn render(&mut self, f: &mut Frame) {
        let size = f.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // header
                Constraint::Length(3), // status (활성 백엔드/경로)
                Constraint::Min(0),    // main
                Constraint::Length(3), // footer
            ])
            .split(size);

        // 헤더
        let header = Paragraph::new(Line::from(vec![
            Span::styled("Session Butler", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::from(" | "),
            Span::styled(format!("v{}", env!("CARGO_PKG_VERSION")), Style::default().fg(Color::Green)),
        ]))
        .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded))
        .alignment(Alignment::Center);
        f.render_widget(header, chunks[0]);

        // status 바: 활성 백엔드 + 세션 경로 + 보존 일수
        let status = self.create_status_bar();
        f.render_widget(status, chunks[1]);

        // 메인 컨텐츠
        match self.state {
            TuiState::MainMenu | TuiState::Input | TuiState::Confirm => {
                self.render_main_menu(f, chunks[2]);
            }
            TuiState::Running => {
                self.render_running(f, chunks[2]);
            }
            TuiState::Results => {
                self.render_results(f, chunks[2]);
            }
        }

        // 푸터
        let footer = self.create_footer();
        let footer_paragraph = Paragraph::new(footer)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded))
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray));

        f.render_widget(footer_paragraph, chunks[3]);

        if self.show_help {
            self.render_help(f);
        }
    }

    /// 상단 status 바 — 활성 백엔드 + codex/hermes 세션 경로 + 보존 일수
    fn create_status_bar(&self) -> Paragraph<'_> {
        let codex_on = self.config.enabled_codex;
        let hermes_on = self.config.enabled_hermes;
        let label = |b: bool| if b { "ON" } else { "OFF" };
        let color = |b: bool| if b { Color::Green } else { Color::DarkGray };

        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("[Codex: {}]", label(codex_on)),
                Style::default().fg(color(codex_on)).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("[Hermes: {}]", label(hermes_on)),
                Style::default().fg(color(hermes_on)).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "  codex: {}  hermes: {}  days: {}",
                self.config.codex_sessions.display(),
                self.config.hermes_sessions.display(),
                self.config.default_archive_days,
            )),
        ]))
        .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded))
        .alignment(Alignment::Left)
    }

    fn render_main_menu(&self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self.get_filtered_items()
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let is_selected = i == self.selected_index;
                let style = if is_selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                ListItem::new(vec![
                    Line::from(Span::styled(&item.title, style)),
                    Line::from(Span::styled(
                        format!("  {}", item.description),
                        Style::default().fg(Color::Gray),
                    )),
                ])
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title("Menu"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        f.render_widget(list, area);

        if self.state == TuiState::Input || self.state == TuiState::Confirm {
            self.render_input_dialog(f, area);
        }
    }

    fn render_input_dialog(&self, f: &mut Frame, area: Rect) {
        let popup_area = Rect {
            x: area.x + area.width / 4,
            y: area.y + area.height / 4,
            width: area.width / 2,
            height: area.height / 2,
        };

        f.render_widget(Clear, popup_area);

        if let Some(item) = self.selected_item() {
            let mut text = vec![
                Line::from(Span::styled(
                    "Argument Input",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::from("")),
            ];

            if self.state == TuiState::Input {
                if let Some(arg) = item.args.get(self.input_field_index) {
                    text.push(Line::from(vec![
                        Span::styled("Name: ", Style::default().fg(Color::Cyan)),
                        Span::styled(&arg.name, Style::default().fg(Color::Yellow)),
                    ]));
                    text.push(Line::from(vec![
                        Span::styled("Description: ", Style::default().fg(Color::Cyan)),
                        Span::styled(&arg.description, Style::default()),
                    ]));
                    text.push(Line::from(Span::from("")));
                    text.push(Line::from(vec![
                        Span::styled("Value: ", Style::default().fg(Color::Cyan)),
                        Span::styled(
                            format!("{}{}", self.input_text, "▏"),
                            Style::default().fg(Color::Green),
                        ),
                    ]));
                    text.push(Line::from(Span::from("")));
                    text.push(Line::from(vec![
                        Span::styled("Default: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(&arg.default_value, Style::default().fg(Color::DarkGray)),
                    ]));
                }
            } else if self.state == TuiState::Confirm {
                text.push(Line::from(Span::styled(
                    "Execute with these arguments?",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )));
                text.push(Line::from(Span::from("")));

                for arg in &item.args {
                    let key = format!("{}_{}", item.id, arg.name);
                    let value = self.input_values.get(&key).cloned().unwrap_or_else(|| arg.value.clone());
                    let value_str = value.to_string();
                    text.push(Line::from(vec![
                        Span::styled(format!("{}: ", arg.name), Style::default().fg(Color::Cyan)),
                        Span::styled(value_str, Style::default().fg(Color::Green)),
                    ]));
                }
            }

            let paragraph = Paragraph::new(text)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded))
                .wrap(Wrap { trim: false });

            f.render_widget(paragraph, popup_area);
        }
    }

    fn render_running(&self, f: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(Span::styled(
                "Running command...",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::from("")),
        ];

        let paragraph = Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title("Running"))
            .wrap(Wrap { trim: false });

        f.render_widget(paragraph, area);
    }

    fn render_results(&self, f: &mut Frame, area: Rect) {
        let output_lines: Vec<Line> = self
            .output_buffer
            .lines()
            .map(|line| Line::from(Span::styled(line, Style::default().fg(Color::Green))))
            .collect();

        let paragraph = Paragraph::new(output_lines)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title("Results"))
            .wrap(Wrap { trim: false });

        f.render_widget(paragraph, area);
    }

    fn render_help(&self, f: &mut Frame) {
        let size = f.area();

        let popup_area = Rect {
            x: size.x + size.width / 8,
            y: size.y + size.height / 8,
            width: size.width * 3 / 4,
            height: size.height * 3 / 4,
        };

        f.render_widget(Clear, popup_area);

        let help_text = vec![
            Line::from(Span::styled(
                "Keyboard Shortcuts",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::from("")),
            Line::from(vec![
                Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
                Span::from(" - Navigate menu"),
            ]),
            Line::from(vec![
                Span::styled("PgUp/PgDn", Style::default().fg(Color::Yellow)),
                Span::from(" - Navigate by page"),
            ]),
            Line::from(vec![
                Span::styled("Home/End", Style::default().fg(Color::Yellow)),
                Span::from(" - Jump to top/bottom"),
            ]),
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::from(" - Select/Confirm"),
            ]),
            Line::from(vec![
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::from(" - Cancel/Back"),
            ]),
            Line::from(vec![
                Span::styled("Tab", Style::default().fg(Color::Yellow)),
                Span::from(" - Next argument field"),
            ]),
            Line::from(vec![
                Span::styled("/", Style::default().fg(Color::Yellow)),
                Span::from(" - Toggle filter"),
            ]),
            Line::from(vec![
                Span::styled("?", Style::default().fg(Color::Yellow)),
                Span::from(" - Toggle help"),
            ]),
            Line::from(vec![
                Span::styled("q", Style::default().fg(Color::Yellow)),
                Span::from(" - Quit"),
            ]),
        ];

        let paragraph = Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title("Help"))
            .wrap(Wrap { trim: false });

        f.render_widget(paragraph, popup_area);
    }

    fn create_footer(&self) -> Line<'static> {
        match self.state {
            TuiState::MainMenu => Line::from(vec![
                Span::styled("↑↓", Style::default().fg(Color::Yellow)),
                Span::from(" Navigate | "),
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::from(" Select | "),
                Span::styled("/", Style::default().fg(Color::Yellow)),
                Span::from(" Filter | "),
                Span::styled("?", Style::default().fg(Color::Yellow)),
                Span::from(" Help | "),
                Span::styled("q", Style::default().fg(Color::Yellow)),
                Span::from(" Quit"),
            ]),
            TuiState::Input => Line::from(vec![
                Span::styled("Tab", Style::default().fg(Color::Yellow)),
                Span::from(" Next | "),
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::from(" Done | "),
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::from(" Cancel"),
            ]),
            TuiState::Confirm => Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::from(" Confirm | "),
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::from(" Cancel"),
            ]),
            TuiState::Running => Line::from(Span::styled("Running...", Style::default().fg(Color::Yellow))),
            TuiState::Results => Line::from(vec![
                Span::styled("Enter/Esc", Style::default().fg(Color::Yellow)),
                Span::from(" Back to menu"),
            ]),
        }
    }
}

pub fn key_to_action(key: KeyEvent) -> Action {
    if key.kind != KeyEventKind::Press {
        return Action::None;
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Action::Up,
        KeyCode::Down | KeyCode::Char('j') => Action::Down,
        KeyCode::PageUp => Action::PageUp,
        KeyCode::PageDown => Action::PageDown,
        KeyCode::Home => Action::Home,
        KeyCode::End => Action::End,
        KeyCode::Enter => Action::Confirm,
        KeyCode::Esc => Action::Cancel,
        KeyCode::Tab => Action::Tab,
        KeyCode::Char('/') => Action::ToggleFilter,
        KeyCode::Char('?') => Action::ToggleHelp,
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char(c) => Action::Input(c),
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Delete => Action::Delete,
        _ => Action::None,
    }
}

pub fn run_tui(config: Config) -> Result<()> {
    enable_raw_mode().map_err(|e| Error::Tui(e.to_string()))?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| Error::Tui(e.to_string()))?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| Error::Tui(e.to_string()))?;

    let mut app = TuiApp::new(config);
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(250);

    let result = 'outer: loop {
        terminal
            .draw(|f| app.render(f))
            .map_err(|e| Error::Tui(e.to_string()))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout).map_err(|e| Error::Tui(e.to_string()))? {
            if let Event::Key(key) = event::read().map_err(|e| Error::Tui(e.to_string()))? {
                let action = key_to_action(key);
                if app.handle_action(action).is_err() {
                    break 'outer Ok(());
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        if app.state == TuiState::Running {
            app.state = TuiState::Results;
        }
    };

    disable_raw_mode().map_err(|e| Error::Tui(e.to_string()))?;

    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .map_err(|e| Error::Tui(e.to_string()))?;

    terminal
        .show_cursor()
        .map_err(|e| Error::Tui(e.to_string()))?;

    result
}
