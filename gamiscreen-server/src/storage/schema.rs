// @generated automatically by Diesel CLI or defined manually
diesel::table! {
    children (id) {
        id -> Text,
        display_name -> Text,
    }
}

diesel::table! {
    tasks (id) {
        id -> Text,
        name -> Text,
        minutes -> Integer,
    }
}

diesel::table! {
    rewards (id) {
        id -> Integer,
        child_id -> Text,
        task_id -> Nullable<Text>,
        minutes -> Integer,
        description -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    sessions (jti) {
        jti -> Text,
        username -> Text,
        issued_at -> Timestamp,
        last_used_at -> Timestamp,
    }
}

diesel::table! {
    usage_minutes (child_id, minute_ts, device_id) {
        child_id -> Text,
        minute_ts -> BigInt,
        device_id -> Text,
    }
}

diesel::table! {
    task_completions (id) {
        id -> Integer,
        child_id -> Text,
        task_id -> Text,
        by_username -> Text,
        done_at -> Timestamp,
    }
}

diesel::joinable!(rewards -> children (child_id));
diesel::joinable!(rewards -> tasks (task_id));

diesel::allow_tables_to_appear_in_same_query!(
    children,
    rewards,
    tasks,
    sessions,
    task_completions,
    usage_minutes,
);
