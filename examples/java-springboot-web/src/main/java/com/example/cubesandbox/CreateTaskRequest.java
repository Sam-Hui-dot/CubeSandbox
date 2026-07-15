package com.example.cubesandbox;

import java.util.Map;

public record CreateTaskRequest(String title, Map<String, Object> payload) {
    public static CreateTaskRequest empty() {
        return new CreateTaskRequest(null, null);
    }
}
