# GitHub Actions Workflows

This directory contains the CI/CD workflows for torrent-combine.

## 🚀 Workflows Overview

### Core Workflows

| Workflow | Purpose | Triggers | Platforms |
|----------|---------|----------|----------|
| **test.yml** | Fast feedback testing | PRs, main pushes | Ubuntu |
| **security.yml** | Security auditing | PRs, main pushes, monthly | Ubuntu |
| **bench.yml** | Performance benchmarks | main pushes | Ubuntu |
| **integration.yml** | Cross-platform testing | PRs, main pushes | Ubuntu, macOS, Windows |
| **coverage.yml** | Code coverage reporting | PRs, main pushes | Ubuntu |
| **release.yml** | Automated releases | Tags | Ubuntu |
| **badge.yml** | Status badge | PRs, main pushes | Ubuntu |
| **ci.yml** | Comprehensive pipeline | PRs, main pushes | Ubuntu |

## 📋 Workflow Details

### `test.yml` - Fast Testing
- **Purpose**: Quick feedback on code changes
- **Jobs**: Build, test, fmt check, clippy
- **Runtime**: ~2-3 minutes
- **Cache**: Cargo registry, index, and build cache

### `security.yml` - Security Auditing
- **Purpose**: Vulnerability scanning
- **Jobs**: Security audit with `cargo audit`
- **Schedule**: Monthly scans + PR/main triggers
- **Alerts**: Fails on security advisories

### `bench.yml` - Performance Testing
- **Purpose**: Performance benchmarking
- **Jobs**: Run cargo bench
- **Triggers**: Main branch pushes only
- **Artifacts**: Benchmark results uploaded

### `integration.yml` - Cross-Platform Testing
- **Purpose**: Multi-platform compatibility
- **Jobs**: Build release binary + integration tests
- **Platforms**: Ubuntu, macOS, Windows
- **Tests**: Real-world usage scenarios

### `coverage.yml` - Code Coverage
- **Purpose**: Test coverage tracking
- **Jobs**: Generate coverage with `cargo llvm-cov`
- **Integration**: Codecov reporting
- **Branch Coverage**: Main branch + PRs

### `release.yml` - Release Automation
- **Purpose**: Automated releases
- **Triggers**: Version tags (v*)
- **Jobs**: Build release binary + create GitHub release
- **Artifacts**: Release archives

## 🔧 Configuration

### Caching Strategy
All workflows use cargo caching for performance:
- `~/.cargo/registry/cache` - Crate registry cache
- `~/.cargo/git/db` - Git index cache
- `target/` - Build artifacts cache

### Rust Toolchain
- Uses `dtolnay/rust-toolchain` action
- Stable toolchain with additional components:
  - `rustfmt` for code formatting
  - `clippy` for linting
  - `llvm-tools-preview` for coverage

### Test Matrix
Some workflows use matrix strategy for:
- **Rust versions**: stable, beta, minimum supported
- **Operating systems**: Ubuntu, macOS, Windows
- **Features**: All feature combinations

## 📊 Monitoring

### Status Badges
- **CI Status**: ![CI](https://github.com/mason-larobina/torrent-combine/workflows/test/badge.svg)
- **Coverage**: ![Coverage](https://codecov.io/gh/mason-larobina/torrent-combine/branch/main/graph/badge.svg)
- **License**: ![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)
- **Crates.io**: ![Crates.io](https://img.shields.io/crates/v/torrent-combine.svg)

### Artifacts
- **Benchmark Results**: Performance data and reports
- **Release Archives**: Binary distributions
- **Coverage Reports**: LCOV format coverage data

## 🛡️ Security

### Automated Scanning
- **Dependency Audits**: `cargo audit` for known vulnerabilities
- **Monthly Scans**: Regular security checks
- **PR Validation**: Security checks on all changes

### Access Control
- **GitHub Token**: Used for security audit API
- **Minimal Permissions**: Only necessary access granted

## 🚀 Performance

### Build Times
- **Cached builds**: ~30 seconds
- **Uncached builds**: ~2-3 minutes
- **Cross-platform**: ~5-10 minutes total

### Optimization
- **Parallel Jobs**: Multiple workflows run concurrently
- **Smart Caching**: Cache invalidation based on Cargo.lock changes
- **Fast Feedback**: Test workflow provides quick PR feedback

## 📝 Integration

### Branch Protection
Recommended branch protection rules:
- **Require CI status checks**: All workflows must pass
- **Require status checks**: Security audit, tests, formatting
- **Require up-to-date**: Branch must be up-to-date with main

### Pull Request Process
1. **Automated Checks**: All workflows trigger automatically
2. **Status Checks**: Results appear in PR interface
3. **Merge Requirements**: All checks must pass before merge
4. **Coverage Tracking**: Coverage trends monitored

## 🔧 Maintenance

### Workflow Updates
- **Dependencies**: Keep actions up to date
- **Rust Versions**: Update minimum supported version as needed
- **Test Coverage**: Add coverage for new features
- **Benchmarks**: Update benchmarks for performance tracking

### Monitoring
- **Workflow Health**: Monitor for failures and timeouts
- **Performance**: Track build times and optimize caching
- **Coverage**: Maintain or improve test coverage
- **Security**: Address security advisories promptly

## 📚 Documentation

- **Workflow README**: This file
- **Main README**: Project documentation with CI/CD section
- **GitHub Actions**: Official GitHub Actions documentation
- **Rust Toolchain**: Rust toolchain and action documentation
