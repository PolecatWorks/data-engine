*** Settings ***
Library    RequestsLibrary

*** Variables ***
${BE_POD_IP}
${EMPTY}

*** Test Cases ***
Backend Direct Pod Health Check
    [Documentation]    Verify the backend health endpoint responds directly on the pod IP.
    Skip If    '${BE_POD_IP}' == '${EMPTY}'    BE_POD_IP is empty
    ${response}=    GET    http://${BE_POD_IP}:8080/api/v1/health    expected_status=200
    Log    Health Response: ${response.content}
    Should Be Equal As Strings    ${response.json()['status']}    ok
    Should Be Equal As Strings    ${response.json()['service']}    data-engine-be

Backend Service Health Check
    [Documentation]    Verify the backend health endpoint responds via the ClusterIP service.
    ${response}=    GET    ${BE_BASE_URL}/api/v1/health    expected_status=200
    Log    Service Health Response: ${response.content}
    Should Be Equal As Strings    ${response.json()['status']}    ok
    Should Be Equal As Strings    ${response.json()['service']}    data-engine-be

Backend Configured Prefix Health Check
    [Documentation]    Verify the backend health endpoint responds via the configured api_prefix.
    ${response}=    GET    ${BE_BASE_URL}/api/data-engine/v1/health    expected_status=200
    Log    Prefix Health Response: ${response.content}
    Should Be Equal As Strings    ${response.json()['status']}    ok
    Should Be Equal As Strings    ${response.json()['service']}    data-engine-be
