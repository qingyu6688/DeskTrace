//! 列表、概览与详情查询。候选状态和覆盖每次按当前阈值与时刻计算，不写回数据库。

use std::cmp::Ordering;
use std::collections::HashMap;

use desktrace_core::{
    CandidateInput, CandidateResult, CandidateStatus, CollectorModule, CoverageStatus, DAY_MS,
    IntervalReason, IntervalSpan, IntervalState, ObjectType, StatusReason, TRACK_BUCKET_MS,
    classify, coverage, coverage_holes, format_utc, local_usage_days, parse_date,
};
use serde::Serialize;

use crate::intervals::GapRecord;
use crate::objects::{ExclusionRef, ObjectRecord};
use crate::scopes::FolderScope;
use crate::{Storage, StorageError};

/// 详情默认返回的近期事件条数。
pub const DETAIL_EVENT_LIMIT: u32 = 100;
/// 概览展示的缺口条数。
pub const OVERVIEW_GAP_LIMIT: u32 = 20;
/// 详情轨迹最多列出的缺口条数，超出部分由前端折叠。
pub const TRACK_GAP_LIMIT: u32 = 200;
/// 区间之间没有任何证据的空档使用的原因值，不属于数据库原因枚举。
pub const UNVERIFIED_REASON: &str = "unverified";

/// 列表排序字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortField {
    /// 名称。
    DisplayName,
    /// 最近直接观察时间。
    LastUsedAt,
    /// 首次发现时间。
    FirstSeenAt,
    /// 安装/更新日期。
    InstallationDate,
}

/// 已校验的列表查询。
#[derive(Clone, Debug)]
pub struct ObjectQuery {
    /// 对象类型，可以多个，例如桌面页同时列出文件和文件夹。
    pub object_types: Vec<ObjectType>,
    /// 名称或路径关键词，忽略大小写。
    pub keyword: Option<String>,
    /// 状态筛选，空表示全部。
    pub statuses: Vec<CandidateStatus>,
    /// 覆盖筛选。
    pub coverage: Option<CoverageStatus>,
    /// 阈值天数。
    pub threshold_days: u32,
    /// 排序字段。
    pub sort_by: SortField,
    /// 是否降序。
    pub descending: bool,
    /// 页码，从 1 开始。
    pub page: u32,
    /// 每页条数。
    pub page_size: u32,
}

/// 分页结果。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageResult<T> {
    /// 当前页数据。
    pub items: Vec<T>,
    /// 匹配总数。
    pub total: usize,
    /// 页码。
    pub page: u32,
    /// 每页条数。
    pub page_size: u32,
}

/// 列表中的一个对象，字段与接口设计文档的 `ObjectSummary` 对应。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectSummary {
    /// 对象 ID。
    pub id: String,
    /// 类型。
    pub object_type: ObjectType,
    /// 展示名称。
    pub display_name: String,
    /// 当前路径或入口。
    pub current_path: Option<String>,
    /// 首次发现。
    pub first_seen_at: String,
    /// 当前观察起点。
    pub observation_started_at: Option<String>,
    /// 最近直接观察。
    pub last_used_at: Option<String>,
    /// 候选状态。
    pub candidate_status: CandidateStatus,
    /// 记录覆盖。
    pub coverage_status: CoverageStatus,
    /// 状态说明。
    pub status_reason: String,
    /// 观察不足时还需的天数。
    pub remaining_observation_days: Option<u32>,
    /// 安装/更新日期。
    pub installation_date: Option<String>,
    /// 日期精度。
    pub installation_precision: String,
    /// 日期含义。
    pub installation_meaning: String,
    /// 日期来源。
    pub installation_source: Option<String>,
    /// 日期校验结果，用于区分缺失和非法。
    pub installation_validation: Option<String>,
    /// 发布者。
    pub publisher: Option<String>,
    /// 版本。
    pub display_version: Option<String>,
    /// 应用类型。
    pub application_kind: Option<String>,
    /// 命中的排除规则。
    pub exclusion: Option<ExclusionRef>,
    #[serde(skip)]
    last_used_at_ms: Option<i64>,
    #[serde(skip)]
    first_seen_at_ms: i64,
}

/// 一类对象的状态计数。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeCounts {
    /// 对象总数。
    pub total: usize,
    /// 各状态数量，七种状态都会出现。
    pub counts: HashMap<CandidateStatus, usize>,
}

/// 概览数据。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// 计算所用阈值。
    pub threshold_days: u32,
    /// 应用计数。
    pub applications: TypeCounts,
    /// 文件夹计数。
    pub folders: TypeCounts,
    /// 文件计数。
    pub files: TypeCounts,
    /// 最早开始记录的时刻。
    pub observation_started_at: Option<String>,
    /// 最近的记录缺口。
    pub recent_gaps: Vec<GapRecord>,
    /// 观察范围。
    pub scopes: Vec<FolderScope>,
}

/// 详情中的观察轨迹，定义见接口设计文档第 4 节。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationTrack {
    /// 轨迹起点。
    pub window_start: String,
    /// 轨迹终点。
    pub window_end: String,
    /// 有直接记录的本地日期。
    pub usage_days: Vec<String>,
    /// 最近直接观察，可能早于起点。
    pub last_used_at: Option<String>,
    /// 与窗口相交的缺口和未知区间。
    pub gaps: Vec<GapRecord>,
}

/// 详情中的一条事件。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventView {
    /// 观察时刻。
    pub observed_at: String,
    /// 事件类型。
    pub event_kind: String,
    /// 中文说明。
    pub label: &'static str,
}

/// 详情中的一条日期证据。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceView {
    /// 来源。
    pub source: String,
    /// 原始值。
    pub raw_value: Option<String>,
    /// 日期值。
    pub date_value: Option<String>,
    /// 精度。
    pub precision: String,
    /// 含义。
    pub meaning: String,
    /// 校验结果。
    pub validation: String,
    /// 读取时刻。
    pub observed_at: String,
}

/// 详情中的一个身份标识。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentifierView {
    /// 标识类型。
    pub kind: String,
    /// 标识值；文件 ID 这类不透明值不返回。
    pub value: Option<String>,
    /// 匹配质量。
    pub quality: String,
    /// 来源。
    pub source: String,
}

/// 对象详情。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectDetail {
    /// 与列表一致的摘要。
    pub summary: ObjectSummary,
    /// 身份标识。
    pub identifiers: Vec<IdentifierView>,
    /// 日期证据，新的在前。
    pub evidence: Vec<EvidenceView>,
    /// 近期事件，新的在前。
    pub recent_events: Vec<EventView>,
    /// 观察轨迹，没有观察起点时为空。
    pub track: Option<ObservationTrack>,
}

/// 按模块缓存区间，同一起点的覆盖只算一次。
struct Classifier {
    now_ms: i64,
    threshold_days: u32,
    /// 系统当前关闭了“最近使用的项目”，文件对象不能下“未使用”类结论。
    file_tracking_disabled: bool,
    spans: HashMap<CollectorModule, Vec<IntervalSpan>>,
    cache: HashMap<(CollectorModule, i64), CoverageStatus>,
}

impl Classifier {
    fn new(storage: &Storage, now_ms: i64, threshold_days: u32) -> Result<Self, StorageError> {
        let mut spans = HashMap::new();
        for module in CollectorModule::ALL {
            spans.insert(module, storage.interval_spans(module)?);
        }
        let file_tracking_disabled = storage.open_interval_reason(CollectorModule::Files)?
            == Some(IntervalReason::SourceDisabled);
        Ok(Self {
            now_ms,
            threshold_days,
            file_tracking_disabled,
            spans,
            cache: HashMap::new(),
        })
    }

    fn summarize(&mut self, record: ObjectRecord) -> ObjectSummary {
        let mut result = classify(&CandidateInput {
            object_type: record.object_type,
            excluded: record.exclusion.is_some(),
            availability: record.availability,
            capability: record.capability,
            observation_started_at_ms: record.observation_started_at_ms,
            last_used_at_ms: record.last_used_at_ms,
            now_ms: self.now_ms,
            threshold_days: self.threshold_days,
        });
        // 来源被系统关闭时，文件的“较久未记录/未记录/观察不足”都没有依据，改为无法判断。
        if self.file_tracking_disabled
            && record.object_type == ObjectType::File
            && matches!(
                result.status,
                CandidateStatus::StaleCandidate
                    | CandidateStatus::NeverObserved
                    | CandidateStatus::InsufficientObservation
            )
        {
            result = CandidateResult {
                status: CandidateStatus::Unknown,
                reason: StatusReason::FileTrackingDisabled,
            };
        }
        let coverage_status = match record.observation_started_at_ms {
            Some(start) => {
                let module = record.module();
                let spans = self.spans.get(&module).map_or(&[][..], Vec::as_slice);
                let now = self.now_ms;
                *self
                    .cache
                    .entry((module, start))
                    .or_insert_with(|| coverage(start, now, spans))
            }
            None => CoverageStatus::Unknown,
        };
        let remaining = match result.reason {
            StatusReason::NeedsMoreDays(days) => Some(days),
            _ => None,
        };
        let evidence = record.installation;
        ObjectSummary {
            first_seen_at: format_utc(record.first_seen_at_ms),
            observation_started_at: record.observation_started_at_ms.map(format_utc),
            last_used_at: record.last_used_at_ms.map(format_utc),
            candidate_status: result.status,
            coverage_status,
            status_reason: result.reason.label(self.threshold_days),
            remaining_observation_days: remaining,
            installation_date: evidence.as_ref().and_then(|value| value.date_value.clone()),
            installation_precision: evidence
                .as_ref()
                .map_or_else(|| "unknown".into(), |value| value.precision.clone()),
            installation_meaning: evidence
                .as_ref()
                .map_or_else(|| "unknown".into(), |value| value.meaning.clone()),
            installation_source: evidence.as_ref().map(|value| value.source.clone()),
            installation_validation: evidence.as_ref().map(|value| value.validation.clone()),
            last_used_at_ms: record.last_used_at_ms,
            first_seen_at_ms: record.first_seen_at_ms,
            id: record.id,
            object_type: record.object_type,
            display_name: record.display_name,
            current_path: record.current_path,
            publisher: record.publisher,
            display_version: record.display_version,
            application_kind: record.application_kind,
            exclusion: record.exclusion,
        }
    }
}

impl Storage {
    /// 按查询条件返回一页对象。
    pub fn list_objects(
        &self,
        query: &ObjectQuery,
        now_ms: i64,
    ) -> Result<PageResult<ObjectSummary>, StorageError> {
        let matches = self.matching_objects(query, now_ms)?;
        let total = matches.len();
        let page_size = query.page_size.max(1);
        let skip = usize::try_from(u64::from(query.page.max(1) - 1) * u64::from(page_size))
            .unwrap_or(usize::MAX);
        let items = matches
            .into_iter()
            .skip(skip)
            .take(page_size as usize)
            .collect();
        Ok(PageResult {
            items,
            total,
            page: query.page.max(1),
            page_size,
        })
    }

    /// 返回全部匹配对象，供导出使用；分页参数被忽略。
    pub fn matching_objects(
        &self,
        query: &ObjectQuery,
        now_ms: i64,
    ) -> Result<Vec<ObjectSummary>, StorageError> {
        let mut classifier = Classifier::new(self, now_ms, query.threshold_days)?;
        let keyword = query
            .keyword
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_lowercase);
        let mut records = Vec::new();
        for object_type in &query.object_types {
            records.extend(self.object_records(*object_type)?);
        }
        let mut items: Vec<ObjectSummary> = records
            .into_iter()
            .filter(|record| match &keyword {
                Some(keyword) => {
                    record.display_name.to_lowercase().contains(keyword)
                        || record
                            .current_path
                            .as_deref()
                            .is_some_and(|path| path.to_lowercase().contains(keyword))
                }
                None => true,
            })
            .map(|record| classifier.summarize(record))
            .filter(|item| {
                query.statuses.is_empty() || query.statuses.contains(&item.candidate_status)
            })
            .filter(|item| {
                query
                    .coverage
                    .is_none_or(|wanted| item.coverage_status == wanted)
            })
            .collect();
        items.sort_by(|left, right| compare(left, right, query.sort_by, query.descending));
        Ok(items)
    }

    /// 概览：两类对象在同一阈值下的状态计数、最近缺口与观察范围。
    pub fn overview(&self, threshold_days: u32, now_ms: i64) -> Result<Overview, StorageError> {
        let mut classifier = Classifier::new(self, now_ms, threshold_days)?;
        let mut count = |object_type: ObjectType| -> Result<TypeCounts, StorageError> {
            let mut counts: HashMap<CandidateStatus, usize> = CandidateStatus::ALL
                .into_iter()
                .map(|status| (status, 0))
                .collect();
            let records = self.object_records(object_type)?;
            let total = records.len();
            for record in records {
                *counts
                    .entry(classifier.summarize(record).candidate_status)
                    .or_default() += 1;
            }
            Ok(TypeCounts { total, counts })
        };
        let applications = count(ObjectType::Application)?;
        let folders = count(ObjectType::Folder)?;
        let files = count(ObjectType::File)?;
        Ok(Overview {
            threshold_days,
            applications,
            folders,
            files,
            observation_started_at: self.first_collection_start()?.map(format_utc),
            recent_gaps: self.gaps(None, i64::MIN, now_ms, OVERVIEW_GAP_LIMIT)?,
            scopes: self.folder_scopes()?,
        })
    }

    /// 对象详情。`offset_minutes` 给出各时刻的本地偏移，用于按本地日期汇总轨迹。
    pub fn object_detail(
        &self,
        id: &str,
        threshold_days: u32,
        retention_days: u32,
        now_ms: i64,
        offset_minutes: &dyn Fn(i64) -> i32,
    ) -> Result<ObjectDetail, StorageError> {
        let record = self.object_record(id)?.ok_or(StorageError::NotFound)?;
        let module = record.module();
        let started = record.observation_started_at_ms;
        let last_used = record.last_used_at_ms;
        let mut classifier = Classifier::new(self, now_ms, threshold_days)?;
        let summary = classifier.summarize(record);
        let track = match started {
            Some(start) if start <= now_ms => {
                let window_start = start.max(now_ms - i64::from(retention_days) * DAY_MS);
                Some(ObservationTrack {
                    window_start: format_utc(window_start),
                    window_end: format_utc(now_ms),
                    usage_days: local_usage_days(
                        &self.usage_buckets(id, window_start, now_ms)?,
                        offset_minutes,
                    ),
                    last_used_at: last_used.map(format_utc),
                    gaps: self.track_gaps(module, window_start, now_ms)?,
                })
            }
            _ => None,
        };
        Ok(ObjectDetail {
            identifiers: self.identifiers(id)?,
            evidence: self.evidence(id)?,
            recent_events: self.events(id, 0, DETAIL_EVENT_LIMIT)?,
            summary,
            track,
        })
    }

    /// 分页读取事件，新的在前。
    pub fn object_events(
        &self,
        id: &str,
        page: u32,
        page_size: u32,
    ) -> Result<PageResult<EventView>, StorageError> {
        if self.object_record(id)?.is_none() {
            return Err(StorageError::NotFound);
        }
        let total: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM usage_events WHERE object_id=?1",
            [id],
            |row| row.get(0),
        )?;
        let page = page.max(1);
        let items = self.events(id, (page - 1).saturating_mul(page_size), page_size)?;
        Ok(PageResult {
            items,
            total: usize::try_from(total).unwrap_or(0),
            page,
            page_size,
        })
    }

    /// 轨迹缺口：记录在案的缺口与未知区间，加上没有任何区间证据的空档，按开始时间倒序。
    fn track_gaps(
        &self,
        module: CollectorModule,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<GapRecord>, StorageError> {
        let mut gaps = self.gaps(Some(module), from_ms, to_ms, TRACK_GAP_LIMIT)?;
        for (start, end) in coverage_holes(from_ms, to_ms, &self.interval_spans(module)?) {
            gaps.push(GapRecord {
                module,
                state: IntervalState::Unknown,
                reason: UNVERIFIED_REASON,
                reason_label: "未记录到采集状态",
                started_at_ms: start,
                ended_at_ms: Some(end),
                started_at: format_utc(start),
                ended_at: Some(format_utc(end)),
            });
        }
        gaps.sort_by_key(|gap| std::cmp::Reverse(gap.started_at_ms));
        gaps.truncate(TRACK_GAP_LIMIT as usize);
        Ok(gaps)
    }

    fn usage_buckets(&self, id: &str, from_ms: i64, to_ms: i64) -> Result<Vec<i64>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT (observed_at/?4)*?4 FROM usage_events WHERE object_id=?1 AND observed_at>=?2 AND observed_at<=?3",
        )?;
        let rows = statement.query_map(
            rusqlite::params![id, from_ms, to_ms, TRACK_BUCKET_MS],
            |row| row.get(0),
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn events(&self, id: &str, offset: u32, limit: u32) -> Result<Vec<EventView>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT observed_at,event_kind FROM usage_events WHERE object_id=?1 ORDER BY observed_at DESC LIMIT ?2 OFFSET ?3",
        )?;
        let rows = statement.query_map(rusqlite::params![id, limit, offset], |row| {
            let kind: String = row.get(1)?;
            Ok(EventView {
                observed_at: format_utc(row.get(0)?),
                label: event_label(&kind),
                event_kind: kind,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn evidence(&self, id: &str) -> Result<Vec<EvidenceView>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT source,raw_value,date_value,date_precision,date_meaning,validation_status,observed_at FROM installation_evidence WHERE object_id=?1 ORDER BY observed_at DESC LIMIT 20",
        )?;
        let rows = statement.query_map([id], |row| {
            Ok(EvidenceView {
                source: row.get(0)?,
                raw_value: row.get(1)?,
                date_value: row.get(2)?,
                precision: row.get(3)?,
                meaning: row.get(4)?,
                validation: row.get(5)?,
                observed_at: format_utc(row.get(6)?),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn identifiers(&self, id: &str) -> Result<Vec<IdentifierView>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT identifier_kind,identifier_key,match_quality,source FROM object_identifiers WHERE object_id=?1 ORDER BY created_at",
        )?;
        let rows = statement.query_map([id], |row| {
            let kind: String = row.get(0)?;
            let key: String = row.get(1)?;
            Ok(IdentifierView {
                value: (kind != "folder_file_id").then_some(key),
                kind,
                quality: row.get(2)?,
                source: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

fn event_label(kind: &str) -> &'static str {
    match kind {
        "app_foreground" => "进入前台",
        "app_active_sample" => "活跃采样",
        "folder_browsed" => "浏览到该目录",
        "folder_visible_sample" => "目录保持可见",
        _ => "其他记录",
    }
}

/// 排序：未知值无论升降序都排在最后；值相同时按名称、再按 ID 保持稳定。
fn compare(
    left: &ObjectSummary,
    right: &ObjectSummary,
    field: SortField,
    descending: bool,
) -> Ordering {
    let primary = match field {
        SortField::DisplayName => Some(
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase()),
        ),
        SortField::LastUsedAt => {
            known_last(left.last_used_at_ms, right.last_used_at_ms, descending)
        }
        SortField::FirstSeenAt => Some(left.first_seen_at_ms.cmp(&right.first_seen_at_ms)),
        SortField::InstallationDate => {
            known_last(installation_day(left), installation_day(right), descending)
        }
    };
    let ordered = match primary {
        // known_last 已经处理过方向和未知值。
        None => return tie_break(left, right),
        Some(order) if matches!(field, SortField::LastUsedAt | SortField::InstallationDate) => {
            order
        }
        Some(order) if descending => order.reverse(),
        Some(order) => order,
    };
    ordered.then_with(|| tie_break(left, right))
}

fn known_last<T: Ord>(left: Option<T>, right: Option<T>, descending: bool) -> Option<Ordering> {
    Some(match (left, right) {
        (Some(left), Some(right)) if descending => right.cmp(&left),
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => return None,
    })
}

fn tie_break(left: &ObjectSummary, right: &ObjectSummary) -> Ordering {
    left.display_name
        .to_lowercase()
        .cmp(&right.display_name.to_lowercase())
        .then_with(|| left.id.cmp(&right.id))
}

fn installation_day(item: &ObjectSummary) -> Option<i64> {
    item.installation_date
        .as_deref()
        .and_then(|value| value.get(0..10))
        .and_then(parse_date)
}
