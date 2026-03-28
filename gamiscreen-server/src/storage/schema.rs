// @generated automatically by Diesel CLI or defined manually
diesel::table! {
    balances (child_id) {
        child_id -> Text,
        minutes_remaining -> Integer,
        account_balance -> Integer,
    }
}

diesel::table! {
    balance_transactions (id) {
        id -> Integer,
        child_id -> Text,
        amount -> Integer,
        description -> Nullable<Text>,
        related_reward_id -> Nullable<Integer>,
        created_at -> Timestamp,
    }
}

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
        required -> Bool,
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
        is_borrowed -> Bool,
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

diesel::table! {
    task_submissions (id) {
        id -> Integer,
        child_id -> Text,
        task_id -> Text,
        submitted_at -> Timestamp,
    }
}

diesel::table! {
    push_subscriptions (id) {
        id -> Integer,
        tenant_id -> Text,
        child_id -> Text,
        endpoint -> Text,
        p256dh -> Text,
        auth -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
        last_success_at -> Nullable<Timestamp>,
        last_error -> Nullable<Text>,
    }
}

diesel::joinable!(rewards -> children (child_id));
diesel::joinable!(rewards -> tasks (task_id));
diesel::joinable!(push_subscriptions -> children (child_id));
diesel::joinable!(balance_transactions -> children (child_id));
diesel::joinable!(balance_transactions -> rewards (related_reward_id));

diesel::allow_tables_to_appear_in_same_query!(
    balances,
    balance_transactions,
    children,
    rewards,
    tasks,
    sessions,
    task_completions,
    task_submissions,
    push_subscriptions,
    usage_minutes,
);
