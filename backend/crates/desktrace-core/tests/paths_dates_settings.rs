use desktrace_core::{
    DateMeaning, DatePrecision, DateValidation, PathError, ScopeRule, Settings, SettingsError,
    SettingsPatch, TRACK_BUCKET_MS, days_from_civil, display_name, file_open_observable,
    is_descendant, is_same_or_descendant, local_usage_days, normalize_local_path,
    package_install_date, parse_date, parse_registry_install_date, resolve_in_scopes,
    top_level_child,
};

#[test]
fn local_paths_are_normalized() -> Result<(), PathError> {
    let path = normalize_local_path(r"d:/Projects\\旧网站\.\")?;
    assert_eq!(path.display, r"D:\Projects\旧网站");
    assert_eq!(path.key, r"D:\PROJECTS\旧网站");
    assert_eq!(
        normalize_local_path(r"\\?\e:\素材库")?.display,
        r"E:\素材库"
    );
    assert_eq!(normalize_local_path("C:\\")?.key, "C:\\");
    assert_eq!(normalize_local_path(r"C:\Work. ")?.key, r"C:\WORK");
    Ok(())
}

#[test]
fn unsupported_paths_are_rejected() {
    assert_eq!(normalize_local_path(""), Err(PathError::Empty));
    assert_eq!(
        normalize_local_path("Projects"),
        Err(PathError::NotLocalAbsolute)
    );
    assert_eq!(normalize_local_path("D:"), Err(PathError::NotLocalAbsolute));
    assert_eq!(
        normalize_local_path(r"\\server\share"),
        Err(PathError::Network)
    );
    assert_eq!(
        normalize_local_path(r"\\?\UNC\server\share"),
        Err(PathError::Network)
    );
    assert_eq!(
        normalize_local_path(r"D:\a\..\b"),
        Err(PathError::ParentTraversal)
    );
    assert_eq!(
        normalize_local_path(r"D:\a|b"),
        Err(PathError::InvalidCharacter)
    );
}

#[test]
fn subtree_matching_uses_components() {
    assert!(is_same_or_descendant(r"C:\WORK", r"C:\WORK"));
    assert!(is_descendant(r"C:\WORK\A", r"C:\WORK"));
    assert!(!is_same_or_descendant(r"C:\WORKSHOP", r"C:\WORK"));
    assert!(is_descendant(r"C:\WORK", "C:\\"));
    assert!(!is_descendant(r"C:\WORK", r"C:\WORK"));
    assert_eq!(display_name(r"D:\Projects\旧网站"), "旧网站");
    assert_eq!(display_name("D:\\"), "D:\\");
}

#[test]
fn registry_install_date_is_strict() {
    let today = days_from_civil(2026, 10, 11);
    let msi = parse_registry_install_date(Some("20260302"), true, today);
    assert_eq!(msi.date_value.as_deref(), Some("2026-03-02"));
    assert_eq!(
        (msi.precision, msi.meaning),
        (DatePrecision::Day, DateMeaning::InstallOrService)
    );
    assert_eq!(
        parse_registry_install_date(Some("20260302"), false, today).meaning,
        DateMeaning::Registered
    );
    for raw in ["2026/03/02", "20260230", "2026030", "abcdefgh"] {
        let evidence = parse_registry_install_date(Some(raw), true, today);
        assert_eq!(evidence.validation, DateValidation::Invalid, "{raw}");
        assert!(evidence.date_value.is_none());
        assert_eq!(evidence.raw_value.as_deref(), Some(raw));
    }
    assert_eq!(
        parse_registry_install_date(Some("20261012"), true, today).validation,
        DateValidation::Future
    );
    assert_eq!(
        parse_registry_install_date(None, true, today).validation,
        DateValidation::Missing
    );
    assert_eq!(
        parse_registry_install_date(Some("  "), true, today).validation,
        DateValidation::Missing
    );
}

#[test]
fn package_date_is_instant_install_or_update() {
    let evidence = package_install_date(Some(1_000), 10_000);
    assert_eq!(evidence.date_value.as_deref(), Some("1970-01-01T00:00:01Z"));
    assert_eq!(evidence.meaning, DateMeaning::InstallOrUpdate);
    assert_eq!(
        package_install_date(Some(10_000_000), 0).validation,
        DateValidation::Future
    );
    assert_eq!(
        package_install_date(None, 0).validation,
        DateValidation::Missing
    );
}

#[test]
fn settings_patch_validates_all_fields() {
    let base = Settings::default();
    assert_eq!(
        base.apply(SettingsPatch {
            threshold_days: Some(0),
            ..SettingsPatch::default()
        }),
        Err(SettingsError::Threshold)
    );
    assert_eq!(
        base.apply(SettingsPatch {
            event_retention_days: Some(29),
            ..SettingsPatch::default()
        }),
        Err(SettingsError::Retention)
    );
    let updated = base.apply(SettingsPatch {
        threshold_days: Some(3650),
        auto_start_enabled: Some(true),
        ..SettingsPatch::default()
    });
    assert_eq!(
        updated.map(|value| (
            value.threshold_days,
            value.event_retention_days,
            value.auto_start_enabled
        )),
        Ok((3650, 180, true))
    );
}

#[test]
fn usage_days_follow_local_offset() {
    // 2026-10-10 16:00Z 在东八区是 10-11 00:00，在 UTC 仍是 10-10。
    let base = (days_from_civil(2026, 10, 10)) * 86_400_000 + 16 * 3_600_000;
    let buckets = [base - TRACK_BUCKET_MS, base, base + TRACK_BUCKET_MS];
    assert_eq!(
        local_usage_days(&buckets, |_| 480),
        ["2026-10-10", "2026-10-11"]
    );
    assert_eq!(local_usage_days(&buckets, |_| 0), ["2026-10-10"]);
}

#[test]
fn date_text_parsing_rejects_signs_and_invalid_days() {
    assert_eq!(
        parse_date("2026-10-11"),
        Some(days_from_civil(2026, 10, 11))
    );
    assert_eq!(parse_date("+026-10-11"), None);
    assert_eq!(parse_date("2026-02-30"), None);
}

#[test]
fn scope_resolution_attributes_to_top_level_items() -> Result<(), PathError> {
    let desktop = normalize_local_path(r"C:\Users\me\Desktop")?;
    let projects = normalize_local_path(r"D:\Projects")?;
    let scopes = [
        ScopeRule {
            root: &desktop,
            include_descendants: false,
        },
        ScopeRule {
            root: &projects,
            include_descendants: true,
        },
    ];
    let deep = normalize_local_path(r"C:\Users\me\Desktop\旧资料\2024\a.docx")?;
    let matched = resolve_in_scopes(&deep, &scopes).ok_or(PathError::Empty)?;
    assert_eq!(matched.target.display, r"C:\Users\me\Desktop\旧资料");
    assert!(!matched.direct);
    let file = normalize_local_path(r"C:\Users\me\Desktop\报价.xlsx")?;
    assert_eq!(
        resolve_in_scopes(&file, &scopes).map(|found| found.direct),
        Some(true)
    );
    assert_eq!(
        resolve_in_scopes(&desktop, &scopes).map(|found| found.target),
        Some(desktop.clone())
    );
    let nested = normalize_local_path(r"D:\Projects\a\b")?;
    assert_eq!(
        resolve_in_scopes(&nested, &scopes).map(|found| found.target),
        Some(nested.clone())
    );
    assert!(resolve_in_scopes(&normalize_local_path(r"C:\Users\me\Desktop2")?, &scopes).is_none());
    let root = normalize_local_path("E:\\")?;
    let child = normalize_local_path(r"E:\素材\x")?;
    assert_eq!(
        top_level_child(&child, &root).map(|path| path.display),
        Some(r"E:\素材".to_string())
    );
    assert!(top_level_child(&root, &root).is_none());
    Ok(())
}

#[test]
fn shortcut_and_program_opens_are_not_observable() {
    for name in [
        "Chrome.lnk",
        "setup.EXE",
        "安装包.msi",
        "站点.url",
        "run.bat",
    ] {
        assert!(!file_open_observable(name), "{name}");
    }
    for name in ["报价.xlsx", "README", ".gitignore", "照片.JPG"] {
        assert!(file_open_observable(name), "{name}");
    }
}
