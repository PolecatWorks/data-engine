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
