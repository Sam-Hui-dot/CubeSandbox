package com.example.cubesandbox;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Instant;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.UUID;
import org.springframework.http.HttpStatus;
import org.springframework.stereotype.Component;
import org.springframework.web.server.ResponseStatusException;

@Component
public class TaskState {
    private static final Path STATE_DIR = Path.of("/tmp/cubesandbox-spring/state");
    private static final Path STATE_FILE = STATE_DIR.resolve("tasks.json");
    private static final TypeReference<Map<String, Map<String, Object>>> TASKS_TYPE =
            new TypeReference<>() {};

    private final ObjectMapper objectMapper;

    public TaskState(ObjectMapper objectMapper) {
        this.objectMapper = objectMapper;
    }

    public synchronized Map<String, Object> createTask(Map<String, Object> request) {
        Map<String, Map<String, Object>> tasks = readTasks();
        String id = UUID.randomUUID().toString();
        Map<String, Object> task = new LinkedHashMap<>();
        task.put("id", id);
        task.put("title", String.valueOf(request.getOrDefault("title", "demo task")));
        task.put("status", "created");
        task.put("createdAt", Instant.now().toString());
        task.put("payload", request.getOrDefault("payload", Map.of()));
        task.put("stateFile", STATE_FILE.toString());
        tasks.put(id, task);
        writeTasks(tasks);
        return task;
    }

    public synchronized Map<String, Object> getTask(String id) {
        Map<String, Object> task = readTasks().get(id);
        if (task == null) {
            throw new ResponseStatusException(HttpStatus.NOT_FOUND, "Task not found: " + id);
        }
        return task;
    }

    private Map<String, Map<String, Object>> readTasks() {
        if (!Files.exists(STATE_FILE)) {
            return new LinkedHashMap<>();
        }
        try {
            return objectMapper.readValue(STATE_FILE.toFile(), TASKS_TYPE);
        } catch (IOException e) {
            throw new ResponseStatusException(HttpStatus.INTERNAL_SERVER_ERROR, "Failed to read task state", e);
        }
    }

    private void writeTasks(Map<String, Map<String, Object>> tasks) {
        try {
            Files.createDirectories(STATE_DIR);
            Path tempFile = Files.createTempFile(STATE_DIR, "tasks", ".json.tmp");
            objectMapper.writerWithDefaultPrettyPrinter().writeValue(tempFile.toFile(), tasks);
            Files.move(tempFile, STATE_FILE, java.nio.file.StandardCopyOption.REPLACE_EXISTING);
        } catch (IOException e) {
            throw new ResponseStatusException(HttpStatus.INTERNAL_SERVER_ERROR, "Failed to write task state", e);
        }
    }
}
