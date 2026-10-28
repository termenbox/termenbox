---
id: 0042-verification-machine-profile
title: Cheap checks run locally, slow checks only in CI (thin machine profile)
status: superseded
superseded-by: 0108-verification-machine-profile
date: 2026-10-11
tags: [practice-packs, verification, harness, ci]
track: process
accepted-by: Sergey Strebulaev <s@vivantel.dev>
go-getter:
  verify-profile: thin
  verify-where:
    format: local
    lint: local
    typecheck: local
    static-analysis: ci
    build: ci
    test: both
    unit: both
    integration: ci
    e2e: ci
  generated-by: verification-gate@0.3.0
  pack-answer: machine-profile
  pack-option: thin
---

## Decision

Format, lint and typecheck run locally; tests run locally and in CI; static analysis, build, integration and e2e run only in CI. `go-getter verify` and the stop gate run only `local` and `both` checks; `ci` and `both` checks belong to CI (pack verification-gate@0.3.0).

## Why

Thin machines should not run builds, full suites or analyzers locally, yet the quality bar must still hold (decision 0071).
