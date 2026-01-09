# GitHub Actions Workflows

This directory contains the CI/CD workflows for torrent-combine.

## Workflows

### `test.yml`
- **Triggers**: Push to `main`, Pull Requests to `main`
- **Purpose**: Fast feedback on code changes
- **Jobs**:
  - Build project
  - Run all tests
  - Check code formatting (`cargo fmt`)
  - Run clippy lints

### `security.yml`
- **Triggers**: Push to `main`, Pull Requests to `main`, Monthly schedule
- **Purpose**: Security vulnerability scanning
- **Jobs**:
  - Security audit using `cargo audit`

### `bench.yml`
- **Triggers**: Push to `main`
- **Purpose**: Performance benchmarking
- **Jobs**:
  - Run performance benchmarks
  - Upload benchmark results as artifacts

### `integration.yml`
- **Triggers**: Push to `main`, Pull Requests to `main`
- **Purpose**: Cross-platform integration testing
- **Jobs**:
  - Test on Ubuntu, macOS, and Windows
  - Build release binary
  - Test binary functionality with real data

### `ci.yml`
- **Triggers**: Push to `main`, Pull Requests to `main`
- **Purpose**: Comprehensive CI pipeline
- **Jobs**:
  - Multi-version Rust testing
  - Security audit
  - Performance benchmarks
  - Cross-platform integration tests
  - Code coverage reporting

## Caching

All workflows use cargo caching to speed up builds:
- Cargo registry cache
- Cargo index cache
- Build target cache

## Test Coverage

Code coverage is collected on pushes to `main` and uploaded to Codecov for tracking coverage trends.

## Security

- Automated security audits run on all PRs and pushes
- Monthly security scans for vulnerability detection
- Uses `cargo audit` to check for known vulnerabilities in dependencies

## Performance

- Benchmarks run on every push to `main`
- Results are stored as GitHub artifacts
- Performance regressions can be detected over time

## Integration Testing

Integration tests verify the binary works correctly across:
- **Ubuntu Linux**: Primary development platform
- **macOS**: Apple platform compatibility
- **Windows**: Windows platform compatibility

Integration tests include:
- Basic file processing
- Source directory functionality
- Exclude directory functionality
- Caching system
- Dry run mode
- CLI option combinations
