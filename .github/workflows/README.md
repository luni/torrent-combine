# GitHub Actions Workflows

This directory contains the CI/CD workflows for torrent-combine.

## Workflows

### `ci.yml` - Comprehensive CI Pipeline
- **Triggers**: Push to `main`, Pull Requests to `main`
- **Purpose**: Complete CI pipeline with all checks
- **Jobs**:
  - Multi-version Rust testing (stable, beta, 1.81.0)
  - Security audit using `cargo audit`
  - Performance benchmarks with regression detection
  - Cross-platform integration tests (Ubuntu, macOS, Windows)
  - Code coverage reporting

### `coverage.yml` - Code Coverage
- **Triggers**: Push to `main`, Pull Requests to `main`
- **Purpose**: Code coverage reporting
- **Jobs**:
  - Generate coverage report using `cargo llvm-cov`
  - Upload to Codecov with GitHub token

### `release.yml` - Automated Releases
- **Triggers**: Tags (e.g., `v1.0.0`)
- **Purpose**: Automated release process
- **Jobs**:
  - Build release binary
  - Create GitHub release
  - Upload release artifacts

### `badge.yml` - Status Badge
- **Triggers**: Push to `main`, Pull Requests to `main`
- **Purpose**: Generate build status badge
- **Jobs**:
  - Build and test project
  - Update status badge

### `security-audit.yml` - Enhanced Security
- **Triggers**: Push to `main`, Pull Requests to `main`, Monthly schedule
- **Purpose**: Comprehensive security scanning
- **Jobs**:
  - Security audit using `cargo audit`
  - Vulnerable GitHub Actions detection
  - Secret scanning
  - File permission audit

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
- Scans for vulnerable GitHub Actions
- Detects hardcoded secrets and file permission issues

## Performance

- Benchmarks run on every push to `main` and pull requests
- Results are stored as GitHub artifacts
- Performance regression detection for PRs
- Comments on PRs with performance comparisons

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

## Workflow Permissions

All workflows have proper permissions:
- **contents: read**: Access repository contents
- **checks: write**: Post check results and comments
- **pull-requests: write**: Comment on PRs and upload artifacts

## Benefits of Consolidation

- **Reduced Complexity**: Fewer workflows to maintain
- **Consistent Permissions**: Uniform permission handling
- **Better Resource Usage**: More efficient CI runs
- **Easier Maintenance**: Single source of truth for CI logic
- **Comprehensive Testing**: All checks in one place
