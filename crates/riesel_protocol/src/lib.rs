pub enum Request {
    Launch { app_id: String },
    Status { instance_id: String },
    Stop { instance_id: String },
}
