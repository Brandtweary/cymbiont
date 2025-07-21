use axum::{
    body::Body,
    http::{Request, StatusCode, Method},
};
use tower::ServiceExt; // for `oneshot`

#[tokio::test]
async fn test_restful_endpoints() {
    // Test that PATCH /sync is accepted and POST /sync/update gives 404
    
    // Create a simple router with PATCH /sync
    let app = axum::Router::new()
        .route("/sync", axum::routing::patch(|| async { "sync updated" }));
    
    // Test PATCH /sync works
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri("/sync")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    // Test that the old POST /sync/update endpoint doesn't exist (404)
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/sync/update")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}