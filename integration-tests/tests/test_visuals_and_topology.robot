*** Settings ***
Documentation     End-to-End Robot Framework integration tests for Spec 13 (Data Visuals Engine) and Spec 14 (Semantic & Structural Topology Engine).
Library           RequestsLibrary
Library           Collections

*** Variables ***
${BE_BASE_URL}    http://localhost:8080
${SEED_DIR}       /tmp/de-robot-visuals-suite

*** Test Cases ***
Visuals Engine Recommender Query And Saved Visuals BREAD
    [Documentation]    Verifies POST /visuals/recommend, POST /visuals/query (with ProvenanceRun), and saved visuals BREAD lifecycle.
    [Tags]    visuals    spec13

    # 1. Seed & profile orders dataset
    ${seed_body}=    Create Dictionary    output_dir=${SEED_DIR}    seed=${42}    row_count=${40}
    ${seed_resp}=    POST    ${BE_BASE_URL}/api/data-engine/v1/datasets/seed    json=${seed_body}    expected_status=200

    ${prof_body}=    Create Dictionary    dataset_name=orders    parquet_path=${SEED_DIR}/sales_orders.parquet
    ${prof_resp}=    POST    ${BE_BASE_URL}/api/data-engine/v1/datasets/profile    json=${prof_body}    expected_status=200

    # 2. Recommend visuals
    ${rec_body}=    Create Dictionary    dataset_name=orders
    ${rec_resp}=    POST    ${BE_BASE_URL}/api/data-engine/v1/visuals/recommend    json=${rec_body}    expected_status=200
    ${recs}=    Get From Dictionary    ${rec_resp.json()}    recommendations
    Should Not Be Empty    ${recs}

    # 3. Query & hydrate chart with ProvenanceRun
    ${binding}=    Create Dictionary    kind=dataset    dataset_id=orders
    ${spec}=    Create Dictionary    mark=bar
    ${vq_body}=    Create Dictionary    title=Robot Orders Visual    renderer=vega_lite    chart_family=bar    data_binding=${binding}    spec=${spec}
    ${vq_resp}=    POST    ${BE_BASE_URL}/api/data-engine/v1/visuals/query    json=${vq_body}    expected_status=200
    ${prov}=    Get From Dictionary    ${vq_resp.json()}    provenance
    Should Not Be Empty    ${prov['provenance_run_id']}
    Should Be Equal As Integers    ${prov['rendered_rows']}    80

    # 4. Saved visuals BREAD
    ${create_body}=    Create Dictionary    name=Robot Saved Chart    description=E2E Test    renderer=vega_lite    chart_family=bar    primary_dataset_id=orders    data_binding=${binding}    visual_spec=${spec}
    ${create_resp}=    POST    ${BE_BASE_URL}/api/data-engine/v1/visuals    json=${create_body}    expected_status=201
    ${vis_id}=    Set Variable    ${create_resp.json()['id']}
    GET    ${BE_BASE_URL}/api/data-engine/v1/visuals/${vis_id}    expected_status=200
    DELETE    ${BE_BASE_URL}/api/data-engine/v1/visuals/${vis_id}    expected_status=204
    GET    ${BE_BASE_URL}/api/data-engine/v1/visuals/${vis_id}    expected_status=404

Topology Graph Dataset Anatomy Mermaid And MCP Tools
    [Documentation]    Verifies GET /topology/graph, /topology/anatomy/dataset/orders, /topology/mermaid, and MCP visual/topology tools.
    [Tags]    topology    mcp    spec14

    ${topo_params}=    Create Dictionary    lens=semantic_3layer
    ${topo_resp}=    GET    url=${BE_BASE_URL}/api/data-engine/v1/topology/graph    params=${topo_params}    expected_status=200
    Should Not Be Empty    ${topo_resp.json()['nodes']}

    ${anat_resp}=    GET    url=${BE_BASE_URL}/api/data-engine/v1/topology/anatomy/dataset/orders    expected_status=200
    Should Be Equal As Strings    ${anat_resp.json()['canonical_id']}    orders

    ${mm_params}=    Create Dictionary    lens=er_join
    ${mm_resp}=    GET    url=${BE_BASE_URL}/api/data-engine/v1/topology/mermaid    params=${mm_params}    expected_status=200
    Should Be Equal As Strings    ${mm_resp.json()['diagram_kind']}    erDiagram

    # Verify MCP tools/list exposes all 4 new visual & topology tools
    ${rpc_body}=    Create Dictionary    jsonrpc=2.0    id=${1}    method=tools/list
    ${mcp_resp}=    POST    ${BE_BASE_URL}/api/data-engine/v1/mcp    json=${rpc_body}    expected_status=200
    ${tools}=    Set Variable    ${mcp_resp.json()['result']['tools']}
    ${names}=    Create List
    FOR    ${t}    IN    @{tools}
        Append To List    ${names}    ${t['name']}
    END
    List Should Contain Value    ${names}    visual_recommend
    List Should Contain Value    ${names}    visual_render_spec
    List Should Contain Value    ${names}    topology_graph
    List Should Contain Value    ${names}    topology_field_anatomy
