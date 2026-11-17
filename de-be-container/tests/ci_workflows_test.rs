use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> Option<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let parent = manifest_dir.parent()?;
    if parent.join(".github/workflows").exists() {
        Some(parent.to_path_buf())
    } else {
        None
    }
}

#[test]
fn test_sward_warden_workflow_files_exist_and_legacy_absent() {
    let Some(root) = repo_root() else {
        println!("Skipping ci_workflows_test: not running in repository workspace (likely container build)");
        return;
    };
    let workflows_dir = root.join(".github/workflows");
    let expected_files = [
        "de-be-docker-publish.yml",
        "de-fe-docker-publish.yml",
        "de-be-release.yml",
        "de-fe-release.yml",
        "helm-ci-pr.yaml",
        "helm-ci-main.yaml",
        "helm-release.yml",
        "integration-test.yaml",
        "cleanup-dev-packages.yml",
        "prune-gh-pages.yml",
        "readme.md",
    ];

    for file in expected_files {
        let path = workflows_dir.join(file);
        assert!(
            path.is_file(),
            "Expected workflow file {} to exist at {:?}",
            file,
            path
        );
    }

    assert!(
        !workflows_dir.join("ci.yml").exists(),
        "Legacy ci.yml should not exist under sward-warden CI pattern"
    );
    assert!(
        !workflows_dir.join("skip-merge-queue.yml").exists(),
        "Legacy skip-merge-queue.yml should not exist under sward-warden CI pattern"
    );
}

#[test]
fn test_docker_publish_workflows_match_sward_warden_pattern() {
    let Some(root) = repo_root() else {
        println!("Skipping test_docker_publish_workflows_match_sward_warden_pattern: outside repository root");
        return;
    };
    let workflows_dir = root.join(".github/workflows");

    let be_yml = fs::read_to_string(workflows_dir.join("de-be-docker-publish.yml"))
        .expect("read de-be-docker-publish.yml");
    assert!(be_yml.contains("name: DE-BE Docker"));
    assert!(be_yml.contains("merge_group:"));
    assert!(be_yml.contains("check-paths:"));
    assert!(be_yml.contains("check-image:"));
    assert!(be_yml.contains("docker manifest inspect"));
    assert!(be_yml.contains("ubuntu-24.04-arm"));
    assert!(be_yml.contains("push-by-digest=true"));
    assert!(be_yml.contains("-de-be-main-"));
    assert!(be_yml.contains("Apply tags to existing image"));
    assert!(be_yml.contains("docs:"));
    assert!(be_yml.contains("git sparse-checkout set \"docs/rust\""));
    assert!(be_yml.contains("rollout-dev:"));
    assert!(
        be_yml.contains("kubectl rollout restart deployment/data-engine-be -n data-engine-dev")
    );
    assert!(be_yml.contains("name: DE-BE CI Success"));

    let fe_yml = fs::read_to_string(workflows_dir.join("de-fe-docker-publish.yml"))
        .expect("read de-fe-docker-publish.yml");
    assert!(fe_yml.contains("name: DE-FE Docker"));
    assert!(fe_yml.contains("merge_group:"));
    assert!(fe_yml.contains("check-paths:"));
    assert!(fe_yml.contains("check-image:"));
    assert!(fe_yml.contains("docker manifest inspect"));
    assert!(fe_yml.contains("ubuntu-24.04-arm"));
    assert!(fe_yml.contains("-de-fe-main-"));
    assert!(fe_yml.contains("Apply tags to existing image"));
    assert!(fe_yml.contains("rollout-dev:"));
    assert!(fe_yml.contains(
        "kubectl rollout restart deployment/data-engine-fe-nginx-view -n data-engine-dev"
    ));
    assert!(fe_yml.contains("name: DE-FE CI Success"));
}

#[test]
fn test_helm_and_release_workflows_contract() {
    let Some(root) = repo_root() else {
        println!("Skipping test_helm_and_release_workflows_contract: outside repository root");
        return;
    };
    let workflows_dir = root.join(".github/workflows");

    let helm_pr =
        fs::read_to_string(workflows_dir.join("helm-ci-pr.yaml")).expect("read helm-ci-pr.yaml");
    assert!(helm_pr.contains("name: Helm CI (PR)"));
    assert!(helm_pr.contains("helm lint charts/data-engine"));
    assert!(helm_pr.contains("name: Helm CI Success"));

    let helm_main = fs::read_to_string(workflows_dir.join("helm-ci-main.yaml"))
        .expect("read helm-ci-main.yaml");
    assert!(helm_main.contains("name: Helm CI (main)"));
    assert!(helm_main.contains("bsord/helm-push@v4"));
    assert!(helm_main.contains("charts/data-engine/Chart.yaml"));
    assert!(helm_main.contains("name: Helm CI Success"));

    for (file, workflow_name) in [
        ("de-be-release.yml", "name: DE-BE Release"),
        ("de-fe-release.yml", "name: DE-FE Release"),
        ("helm-release.yml", "name: Helm Release"),
    ] {
        let content = fs::read_to_string(workflows_dir.join(file)).expect("read release workflow");
        assert!(content.contains(workflow_name));
        assert!(content.contains("*.*.*"));
        assert!(content.contains("^[0-9]+\\.[0-9]+\\.[0-9]+$"));
    }
}

#[test]
fn test_integration_and_housekeeping_workflows_contract() {
    let Some(root) = repo_root() else {
        println!("Skipping test_integration_and_housekeeping_workflows_contract: outside repository root");
        return;
    };
    let workflows_dir = root.join(".github/workflows");

    let it_yaml = fs::read_to_string(workflows_dir.join("integration-test.yaml"))
        .expect("read integration-test.yaml");
    assert!(it_yaml.contains("name: Integration Tests"));
    assert!(
        it_yaml
            .contains("if: needs.check-paths.outputs.run_tests == 'true' && github.event_name == 'merge_group'")
    );
    assert!(it_yaml.contains("runs-on: polecatworks-runner"));
    assert!(it_yaml.contains("NAMESPACE=\"data-engine-pr-${{ env.PR_NUMBER }}\""));
    assert!(it_yaml.contains("Wait for Docker Images"));
    assert!(it_yaml.contains("garden test --env pr"));
    assert!(it_yaml.contains("git sparse-checkout set \"$TARGET_DIR\""));
    assert!(it_yaml.contains("date +%s > gh-pages-repo/$TARGET_DIR/.timestamp"));
    assert!(it_yaml.contains("Teardown Garden Environment"));
    assert!(it_yaml.contains("garden cleanup namespace --env pr"));
    assert!(it_yaml.contains("name: integration"));

    let cleanup = fs::read_to_string(workflows_dir.join("cleanup-dev-packages.yml"))
        .expect("read cleanup-dev-packages.yml");
    assert!(cleanup.contains("snok/container-retention-policy@v2.2.1"));
    assert!(cleanup.contains("skip-tags: main,latest,*.*.*"));
    assert!(cleanup.contains("image-names: ${{ steps.repo.outputs.repo_name }}/helm/data-engine"));

    let prune = fs::read_to_string(workflows_dir.join("prune-gh-pages.yml"))
        .expect("read prune-gh-pages.yml");
    assert!(prune.contains("name: Prune GitHub Pages Reports"));
    assert!(prune.contains(".timestamp"));
    assert!(prune.contains("7 * 24 * 3600"));
}

#[test]
fn test_dockerfile_helm_chart_and_garden_configs() {
    let Some(root) = repo_root() else {
        println!("Skipping test_dockerfile_helm_chart_and_garden_configs: outside repository root");
        return;
    };

    let dockerfile = fs::read_to_string(root.join("de-be-container/Dockerfile"))
        .expect("read de-be-container/Dockerfile");
    assert!(dockerfile.contains("FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS chef"));
    assert!(dockerfile.contains("FROM buildcacherelease AS build"));
    assert!(dockerfile.contains("RUN cargo test -- --test-threads=1"));
    assert!(dockerfile.contains("RUN cargo build --release"));
    assert!(dockerfile.contains("RUN /usr/local/bin/de-be --help"));

    let chart_yaml = fs::read_to_string(root.join("charts/data-engine/Chart.yaml"))
        .expect("read charts/data-engine/Chart.yaml");
    assert!(chart_yaml.contains("name: data-engine"));

    let garden_yml = fs::read_to_string(root.join("garden.yml")).expect("read garden.yml");
    assert!(garden_yml.contains("name: data-engine"));
    assert!(garden_yml.contains("defaultNamespace: data-engine-pr-${var.pr-number}"));
    assert!(garden_yml.contains("name: get-be-sha"));
    assert!(garden_yml.contains("name: get-fe-sha"));

    assert!(root.join(".gardenignore").is_file());
    assert!(root.join("integration-tests/garden.yml").is_file());

    let makefile = fs::read_to_string(root.join("Makefile")).expect("read Makefile");
    assert!(makefile.contains("de-be-docker"));
    assert!(makefile.contains("squash-gh-pages"));
}
