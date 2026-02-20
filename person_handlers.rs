
// ==================== People endpoints ====================

#[derive(Deserialize)]
struct CreatePersonRequest {
    name: String,
}

async fn create_person_handler(
    State(state): State<AppState>,
    Json(body): Json<CreatePersonRequest>,
) -> Result<impl IntoResponse, DomainError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(DomainError::Io("Person name cannot be empty".to_string()));
    }
    let id = Uuid::new_v4();
    let person = state.repo.create_person(id, name)?;
    Ok((StatusCode::CREATED, Json(person)))
}

async fn list_named_people_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, DomainError> {
    let people = state.repo.list_people()?;
    Ok(Json(people))
}

async fn delete_person_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, DomainError> {
    state.repo.delete_person(id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn rename_person_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreatePersonRequest>,
) -> Result<impl IntoResponse, DomainError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(DomainError::Io("Person name cannot be empty".to_string()));
    }
    state.repo.rename_person(id, name)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct NameFaceRequest {
    person_id: Option<Uuid>,
}

async fn name_face_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<NameFaceRequest>,
) -> Result<impl IntoResponse, DomainError> {
    state.repo.name_face(id, body.person_id)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct NameClusterRequest {
    person_id: Option<Uuid>,
}

async fn name_cluster_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<NameClusterRequest>,
) -> Result<impl IntoResponse, DomainError> {
    state.repo.name_cluster(id, body.person_id)?;
    Ok(StatusCode::OK)
}
