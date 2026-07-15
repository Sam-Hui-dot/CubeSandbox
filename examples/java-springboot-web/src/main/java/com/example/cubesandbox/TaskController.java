package com.example.cubesandbox;

import java.util.Map;
import org.springframework.http.HttpStatus;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.ResponseStatus;
import org.springframework.web.bind.annotation.RestController;

@RestController
@RequestMapping("/api/tasks")
public class TaskController {
    private final TaskState taskState;

    public TaskController(TaskState taskState) {
        this.taskState = taskState;
    }

    @PostMapping
    @ResponseStatus(HttpStatus.CREATED)
    public Map<String, Object> createTask(@RequestBody(required = false) CreateTaskRequest request) {
        return taskState.createTask(request == null ? CreateTaskRequest.empty() : request);
    }

    @GetMapping("/{id}")
    public Map<String, Object> getTask(@PathVariable String id) {
        return taskState.getTask(id);
    }
}
