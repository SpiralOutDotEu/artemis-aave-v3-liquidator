#!/usr/bin/env python3
"""
Simple Web-Based Log Viewer for Artemis Aave V3 Liquidator
Usage: python3 log_viewer.py
Then open http://localhost:5000 in your browser
"""

import os
import glob
import re
import sqlite3
from datetime import datetime
from flask import Flask, render_template_string, request, jsonify
import json

app = Flask(__name__)

# HTML template for the log viewer
HTML_TEMPLATE = """
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <title>Artemis Log Viewer</title>
    <style>
        * {
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }
        
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
            background-color: #f8f9fa;
            color: #333;
            line-height: 1.6;
        }
        
        .container {
            max-width: 1600px;
            margin: 0 auto;
            background: white;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
            border-radius: 8px;
            overflow: hidden;
        }
        
        .header {
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
            padding: 24px;
            text-align: center;
        }
        
        .header h1 {
            font-size: 28px;
            font-weight: 600;
            margin-bottom: 8px;
        }
        
        .header p {
            opacity: 0.9;
            font-size: 16px;
        }
        
        .controls {
            padding: 24px;
            background: #f8f9fa;
            border-bottom: 1px solid #e9ecef;
        }
        
        .control-row {
            display: flex;
            gap: 20px;
            align-items: center;
            margin-bottom: 16px;
        }
        
        .control-row:last-child {
            margin-bottom: 0;
        }
        
        .control-group {
            display: flex;
            flex-direction: column;
            gap: 6px;
        }
        
        .control-group label {
            font-weight: 600;
            color: #495057;
            font-size: 14px;
        }
        
        .control-group select,
        .control-group input[type="text"] {
            padding: 8px 12px;
            border: 1px solid #ced4da;
            border-radius: 6px;
            font-size: 14px;
            min-width: 200px;
            background: white;
        }
        
        .control-group select:focus,
        .control-group input[type="text"]:focus {
            outline: none;
            border-color: #667eea;
            box-shadow: 0 0 0 3px rgba(102, 126, 234, 0.1);
        }
        
        .checkbox-group {
            display: flex;
            align-items: center;
            gap: 8px;
        }
        
        .checkbox-group input[type="checkbox"] {
            width: 18px;
            height: 18px;
            accent-color: #667eea;
        }
        
        .checkbox-group label {
            font-weight: 500;
            color: #495057;
            font-size: 14px;
            cursor: pointer;
        }
        
        .btn {
            padding: 8px 16px;
            border: none;
            border-radius: 6px;
            font-size: 14px;
            font-weight: 500;
            cursor: pointer;
            transition: all 0.2s ease;
        }
        
        .btn-primary {
            background: #667eea;
            color: white;
        }
        
        .btn-primary:hover {
            background: #5a6fd8;
            transform: translateY(-1px);
        }
        
        .stats {
            padding: 20px 24px;
            background: white;
            border-bottom: 1px solid #e9ecef;
        }
        
        .stats-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
            gap: 16px;
        }
        
        .stat-card {
            background: #f8f9fa;
            padding: 16px;
            border-radius: 8px;
            text-align: center;
            border: 1px solid #e9ecef;
        }
        
        .stat-number {
            font-size: 24px;
            font-weight: 700;
            color: #495057;
            margin-bottom: 4px;
        }
        
        .stat-label {
            color: #6c757d;
            font-size: 13px;
            font-weight: 500;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }
        
        .logs-table-container {
            overflow-x: auto;
            background: white;
        }
        
        .logs-table {
            width: 100%;
            border-collapse: collapse;
            font-size: 13px;
            font-family: 'SF Mono', Monaco, 'Cascadia Code', 'Roboto Mono', Consolas, 'Courier New', monospace;
        }
        
        .logs-table th {
            background: #f8f9fa;
            padding: 12px 16px;
            text-align: left;
            font-weight: 600;
            color: #495057;
            border-bottom: 2px solid #dee2e6;
            position: sticky;
            top: 0;
            z-index: 10;
        }
        
        .logs-table th span {
            color: #667eea;
            font-weight: 700;
            margin-left: 4px;
        }
        
        .logs-table td {
            padding: 12px 16px;
            border-bottom: 1px solid #f1f3f4;
            vertical-align: top;
        }
        
        .logs-table tbody tr:hover {
            background-color: #f8f9fa;
        }
        
        .log-level {
            display: inline-block;
            padding: 4px 8px;
            border-radius: 4px;
            font-size: 11px;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.5px;
            min-width: 60px;
            text-align: center;
        }
        
        .log-level.error {
            background-color: #f8d7da;
            color: #721c24;
        }
        
        .log-level.warn {
            background-color: #fff3cd;
            color: #856404;
        }
        
        .log-level.info {
            background-color: #d1ecf1;
            color: #0c5460;
        }
        
        .log-level.debug {
            background-color: #e2e3e5;
            color: #383d41;
        }
        
        .log-timestamp {
            color: #6c757d;
            font-weight: 500;
            white-space: nowrap;
        }
        
        .log-thread {
            color: #6f42c1;
            font-weight: 500;
            font-size: 12px;
        }
        
        .log-module {
            color: #007bff;
            font-weight: 500;
            font-size: 12px;
        }
        
        .log-message {
            color: #212529;
            line-height: 1.5;
            word-break: break-word;
            max-width: 400px;
        }
        
        .pagination {
            padding: 20px 24px;
            background: #f8f9fa;
            border-top: 1px solid #e9ecef;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }
        
        .pagination button {
            padding: 8px 16px;
            border: 1px solid #ced4da;
            background: white;
            cursor: pointer;
            border-radius: 6px;
            font-size: 14px;
            transition: all 0.2s ease;
        }
        
        .pagination button:hover:not(:disabled) {
            background: #e9ecef;
            border-color: #adb5bd;
        }
        
        .pagination button:disabled {
            opacity: 0.5;
            cursor: not-allowed;
        }
        
        .page-info {
            font-weight: 500;
            color: #495057;
        }
        
        .loading {
            text-align: center;
            padding: 40px;
            color: #6c757d;
            font-size: 16px;
        }
        
        .no-logs {
            text-align: center;
            padding: 40px;
            color: #6c757d;
            font-size: 16px;
        }
        
        .notification {
            position: fixed;
            top: 20px;
            right: 20px;
            background: #28a745;
            color: white;
            padding: 16px 20px;
            border-radius: 8px;
            box-shadow: 0 4px 12px rgba(0,0,0,0.15);
            z-index: 1000;
            font-weight: 500;
            display: none;
            animation: slideIn 0.3s ease-out;
        }
        
        @keyframes slideIn {
            from { transform: translateX(100%); opacity: 0; }
            to { transform: translateX(0); opacity: 1; }
        }
        
        @media (max-width: 768px) {
            .control-row {
                flex-direction: column;
                align-items: stretch;
            }
            
            .control-group {
                min-width: auto;
            }
            
            .stats-grid {
                grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
            }
        }

        /* Tab System */
        .tabs {
            display: flex;
            border-bottom: 2px solid #e0e0e0;
            margin-bottom: 20px;
            background: #f8f9fa;
            border-radius: 8px 8px 0 0;
        }
        
        .tab-button {
            padding: 12px 24px;
            border: none;
            background: transparent;
            cursor: pointer;
            font-size: 16px;
            font-weight: 500;
            color: #666;
            border-bottom: 3px solid transparent;
            transition: all 0.3s ease;
        }
        
        .tab-button:hover {
            background: #e9ecef;
            color: #333;
        }
        
        .tab-button.active {
            color: #007bff;
            border-bottom-color: #007bff;
            background: white;
        }
        
        .tab-content {
            display: none;
        }
        
        .tab-content.active {
            display: block;
        }

        /* Borrowers Table Styles */
        .filter-group {
            margin: 10px 20px 10px 0;
            display: inline-block;
        }
        
        .filter-group label {
            display: block;
            margin-bottom: 5px;
            font-weight: 500;
            color: #333;
        }
        
        .filter-group input,
        .filter-group select {
            padding: 8px 12px;
            border: 1px solid #ddd;
            border-radius: 4px;
            font-size: 14px;
            min-width: 150px;
        }
        
        .filter-group input:focus,
        .filter-group select:focus {
            outline: none;
            border-color: #007bff;
            box-shadow: 0 0 0 2px rgba(0,123,255,0.25);
        }
        
        #borrowersTable {
            width: 100%;
            border-collapse: collapse;
            margin-top: 20px;
            background: white;
            border-radius: 8px;
            overflow: hidden;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
        }
        
        #borrowersTable th {
            background: #f8f9fa;
            padding: 12px;
            text-align: left;
            font-weight: 600;
            color: #333;
            border-bottom: 2px solid #e0e0e0;
            cursor: pointer;
            user-select: none;
        }
        
        #borrowersTable th:hover {
            background: #e9ecef;
        }
        
        #borrowersTable td {
            padding: 12px;
            border-bottom: 1px solid #f0f0f0;
            vertical-align: top;
        }
        
        #borrowersTable tbody tr:hover {
            background: #f8f9fa;
        }
        
        .address-cell {
            font-family: 'Courier New', monospace;
            font-size: 12px;
            color: #007bff;
        }
        
        .token-list {
            max-width: 200px;
            word-wrap: break-word;
        }
        
        .token-item {
            display: inline-block;
            background: #e9ecef;
            padding: 2px 6px;
            margin: 2px;
            border-radius: 3px;
            font-size: 11px;
            font-family: 'Courier New', monospace;
            color: #495057;
        }
        
        .status-badge {
            padding: 4px 8px;
            border-radius: 12px;
            font-size: 12px;
            font-weight: 500;
            text-align: center;
            min-width: 80px;
        }
        
        .status-has-collateral {
            background: #d4edda;
            color: #155724;
        }
        
        .status-has-debt {
            background: #f8d7da;
            color: #721c24;
        }
        
        .status-both {
            background: #d1ecf1;
            color: #0c5460;
        }
        
        .status-neither {
            background: #f8f9fa;
            color: #6c757d;
        }
        
        .stats-bar {
            background: #f8f9fa;
            padding: 15px 20px;
            border-radius: 8px;
            margin: 20px 0;
            border: 1px solid #e0e0e0;
        }
        
        #borrowerStats {
            font-weight: 500;
            color: #333;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>🔍 Artemis Log Viewer</h1>
            <p>Real-time log analysis for your Aave V3 Liquidator bot</p>
        </div>
        
        <!-- Navigation Tabs -->
        <div class="tabs">
            <button class="tab-button active" onclick="showTab('logs')">📊 Logs</button>
            <button class="tab-button" onclick="showTab('borrowers')">👥 Borrowers</button>
        </div>

        <!-- Logs Tab -->
        <div id="logs-tab" class="tab-content active">
            <div class="controls">
                <div class="control-row">
                    <div class="control-group">
                        <label for="logFile">Log File</label>
                        <select id="logFile" onchange="loadLogs()">
                            <option value="">All Log Files</option>
                        </select>
                    </div>
                    
                    <div class="control-group">
                        <label for="searchInput">Search</label>
                        <input type="text" id="searchInput" placeholder="Search in logs..." oninput="filterLogs()">
                    </div>
                    
                    <div class="control-group">
                        <label for="levelFilter">Log Level</label>
                        <select id="levelFilter" onchange="filterLogs()">
                            <option value="">All Levels</option>
                            <option value="ERROR">ERROR</option>
                            <option value="WARN">WARN</option>
                            <option value="INFO">INFO</option>
                            <option value="DEBUG">DEBUG</option>
                        </select>
                    </div>
                    
                    <div class="control-group">
                        <label for="threadFilter">Thread</label>
                        <select id="threadFilter" onchange="filterLogs()">
                            <option value="">All Threads</option>
                        </select>
                    </div>
                    
                    <div class="control-group">
                        <label for="sortOrder">Sort Order</label>
                        <select id="sortOrder" onchange="applySorting()">
                            <option value="newest">Newest First</option>
                            <option value="oldest">Oldest First</option>
                        </select>
                    </div>
                </div>
                
                <div class="control-row">
                    <div class="checkbox-group">
                        <input type="checkbox" id="autoRefreshToggle" checked>
                        <label for="autoRefreshToggle">Auto-refresh every 5 seconds</label>
                    </div>
                    
                    <button id="refreshBtn" class="btn btn-primary" onclick="loadLogs()">🔄 Refresh</button>
                </div>
            </div>
            
            <div class="stats">
                <div class="stats-grid">
                    <div class="stat-card">
                        <div class="stat-number" id="totalLogs">0</div>
                        <div class="stat-label">Total Logs</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-number" id="errorCount">0</div>
                        <div class="stat-label">Errors</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-number" id="warnCount">0</div>
                        <div class="stat-label">Warnings</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-number" id="infoCount">0</div>
                        <div class="stat-label">Info</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-number" id="debugCount">0</div>
                        <div class="stat-label">Debug</div>
                    </div>
                </div>
            </div>
            
            <div class="logs-table-container">
                <table class="logs-table">
                    <thead>
                        <tr>
                            <th>Timestamp <span id="sortIndicator">↓</span></th>
                            <th>Level</th>
                            <th>Thread</th>
                            <th>Module</th>
                            <th>Message</th>
                        </tr>
                    </thead>
                    <tbody id="logsTableBody">
                        <tr>
                            <td colspan="5" class="loading">Loading logs...</td>
                        </tr>
                    </tbody>
                </table>
            </div>
            
            <div class="pagination">
                <button id="prevBtn" onclick="previousPage()" disabled>← Previous</button>
                <div class="page-info" id="pageInfo">Page 1 of 1</div>
                <button id="nextBtn" onclick="nextPage()" disabled>Next →</button>
            </div>
        </div>

        <!-- Borrowers Tab -->
        <div id="borrowers-tab" class="tab-content">
            <div class="controls">
                <div class="filter-group">
                    <label for="borrowerSearch">Search Address:</label>
                    <input type="text" id="borrowerSearch" placeholder="Enter address or partial address..." oninput="filterBorrowers()">
                </div>
                <div class="filter-group">
                    <label for="collateralFilter">Collateral Token:</label>
                    <select id="collateralFilter" onchange="filterBorrowers()">
                        <option value="">All Collateral</option>
                    </select>
                </div>
                <div class="filter-group">
                    <label for="debtFilter">Debt Token:</label>
                    <select id="debtFilter" onchange="filterBorrowers()">
                        <option value="">All Debt</option>
                    </select>
                </div>
                <div class="filter-group">
                    <label for="statusFilter">Status:</label>
                    <select id="statusFilter" onchange="filterBorrowers()">
                        <option value="">All Status</option>
                        <option value="has_collateral">Has Collateral</option>
                        <option value="has_debt">Has Debt</option>
                        <option value="both">Both</option>
                        <option value="neither">Neither</option>
                    </select>
                </div>
            </div>

            <div class="stats-bar">
                <span id="borrowerStats">Loading...</span>
            </div>

            <div class="table-container">
                <table id="borrowersTable">
                    <thead>
                        <tr>
                            <th onclick="sortTable('address')">Address ↕</th>
                            <th onclick="sortTable('collateral_count')">Collateral Count ↕</th>
                            <th onclick="sortTable('debt_count')">Debt Count ↕</th>
                            <th>Collateral Tokens</th>
                            <th>Debt Tokens</th>
                            <th>Status</th>
                        </tr>
                    </thead>
                    <tbody id="borrowersTableBody">
                        <tr><td colspan="6">Loading borrowers...</td></tr>
                    </tbody>
                </table>
            </div>
        </div>
    </div>
    
    <div id="newLogsNotification" class="notification"></div>
    
    <script>
        let allLogs = [];
        let filteredLogs = [];
        let currentPage = 1;
        let logsPerPage = 100;
        let autoRefreshInterval;

        // Load log files and populate dropdown
        async function loadLogFiles() {
            try {
                const response = await fetch('/api/log-files');
                const files = await response.json();
                const select = document.getElementById('logFile');
                select.innerHTML = '<option value="">All Log Files</option>';
                files.forEach(file => {
                    const option = document.createElement('option');
                    option.value = file;
                    option.textContent = file;
                    select.appendChild(option);
                });
            } catch (error) {
                console.error('Error loading log files:', error);
            }
        }

        // Load and parse logs
        async function loadLogs() {
            const logFile = document.getElementById('logFile').value;
            try {
                const url = logFile ? `/api/logs?file=${encodeURIComponent(logFile)}` : '/api/logs';
                const response = await fetch(url);
                const data = await response.json();
                
                // Check if we have new logs
                const previousLogCount = allLogs.length;
                allLogs = data.logs;
                
                // Apply current sorting preference
                const sortOrder = document.getElementById('sortOrder').value;
                if (sortOrder === 'newest') {
                    allLogs.sort((a, b) => {
                        const timeA = new Date(a.timestamp).getTime();
                        const timeB = new Date(b.timestamp).getTime();
                        return timeB - timeA; // Newest first
                    });
                } else {
                    allLogs.sort((a, b) => {
                        const timeA = new Date(a.timestamp).getTime();
                        const timeB = new Date(b.timestamp).getTime();
                        return timeA - timeB; // Oldest first
                    });
                }
                
                // Update sort indicator
                const sortIndicator = document.getElementById('sortIndicator');
                if (sortOrder === 'newest') {
                    sortIndicator.textContent = '↓';
                    sortIndicator.title = 'Newest logs first';
                } else {
                    sortIndicator.textContent = '↑';
                    sortIndicator.title = 'Oldest logs first';
                }
                
                filteredLogs = [...allLogs];
                
                // Update thread filter options
                updateThreadFilter();
                
                // Always start at page 1 for consistent behavior
                currentPage = 1;
                
                displayLogs();
                updateStats();
                updatePagination();
                
                // Return whether we have new logs
                return allLogs.length > previousLogCount;
            } catch (error) {
                console.error('Error loading logs:', error);
                document.getElementById('logsTableBody').innerHTML = '<tr><td colspan="5" class="error">Error loading logs</td></tr>';
                return false;
            }
        }

        // Update thread filter options based on available threads
        function updateThreadFilter() {
            const threads = [...new Set(allLogs.map(log => log.thread))].sort();
            const select = document.getElementById('threadFilter');
            select.innerHTML = '<option value="">All Threads</option>';
            threads.forEach(thread => {
                const option = document.createElement('option');
                option.value = thread;
                option.textContent = thread;
                select.appendChild(option);
            });
        }

        // Filter logs based on search criteria
        function filterLogs() {
            const searchTerm = document.getElementById('searchInput').value.toLowerCase();
            const logLevel = document.getElementById('levelFilter').value;
            const threadFilter = document.getElementById('threadFilter').value;
            
            filteredLogs = allLogs.filter(log => {
                const matchesSearch = !searchTerm || 
                    log.message.toLowerCase().includes(searchTerm) ||
                    log.module.toLowerCase().includes(searchTerm) ||
                    log.thread.toLowerCase().includes(searchTerm);
                
                const matchesLevel = !logLevel || log.level === logLevel;
                const matchesThread = !threadFilter || log.thread === threadFilter;
                
                return matchesSearch && matchesLevel && matchesThread;
            });
            
            // Always start at page 1 for consistent behavior
            currentPage = 1;
            
            displayLogs();
            updatePagination();
        }

        // Display logs for current page
        function displayLogs() {
            const startIndex = (currentPage - 1) * logsPerPage;
            const endIndex = startIndex + logsPerPage;
            const pageLogs = filteredLogs.slice(startIndex, endIndex);
            
            const tbody = document.getElementById('logsTableBody');
            tbody.innerHTML = '';
            
            if (pageLogs.length === 0) {
                tbody.innerHTML = '<tr><td colspan="5" class="no-logs">No logs found</td></tr>';
                return;
            }
            
            pageLogs.forEach(log => {
                const row = document.createElement('tr');
                
                const timestampCell = document.createElement('td');
                timestampCell.className = 'log-timestamp';
                timestampCell.textContent = log.timestamp;
                
                const levelCell = document.createElement('td');
                const levelSpan = document.createElement('span');
                levelSpan.className = `log-level ${log.level.toLowerCase()}`;
                levelSpan.textContent = log.level;
                levelCell.appendChild(levelSpan);
                
                const threadCell = document.createElement('td');
                threadCell.className = 'log-thread';
                threadCell.textContent = log.thread;
                
                const moduleCell = document.createElement('td');
                moduleCell.className = 'log-module';
                moduleCell.textContent = log.module;
                
                const messageCell = document.createElement('td');
                messageCell.className = 'log-message';
                messageCell.textContent = log.message;
                
                row.appendChild(timestampCell);
                row.appendChild(levelCell);
                row.appendChild(threadCell);
                row.appendChild(moduleCell);
                row.appendChild(messageCell);
                
                tbody.appendChild(row);
            });
        }

        // Update pagination controls
        function updatePagination() {
            const totalPages = Math.ceil(filteredLogs.length / logsPerPage);
            const pagination = document.querySelector('.pagination');
            
            if (totalPages <= 1) {
                pagination.style.display = 'none';
                return;
            }
            
            pagination.style.display = 'flex';
            document.getElementById('prevBtn').disabled = currentPage === 1;
            document.getElementById('nextBtn').disabled = currentPage === totalPages;
            document.getElementById('pageInfo').textContent = `Page ${currentPage} of ${totalPages}`;
        }

        // Update statistics
        function updateStats() {
            const totalLogs = filteredLogs.length;
            const errorCount = filteredLogs.filter(log => log.level === 'ERROR').length;
            const warnCount = filteredLogs.filter(log => log.level === 'WARN').length;
            const infoCount = filteredLogs.filter(log => log.level === 'INFO').length;
            const debugCount = filteredLogs.filter(log => log.level === 'DEBUG').length;
            
            document.getElementById('totalLogs').textContent = totalLogs;
            document.getElementById('errorCount').textContent = errorCount;
            document.getElementById('warnCount').textContent = warnCount;
            document.getElementById('infoCount').textContent = infoCount;
            document.getElementById('debugCount').textContent = debugCount;
        }

        // Apply sorting based on user selection
        function applySorting() {
            const sortOrder = document.getElementById('sortOrder').value;
            const sortIndicator = document.getElementById('sortIndicator');
            
            // Update sort indicator
            if (sortOrder === 'newest') {
                sortIndicator.textContent = '↓'; // Down arrow for newest first
                sortIndicator.title = 'Newest logs first';
            } else {
                sortIndicator.textContent = '↑'; // Up arrow for oldest first
                sortIndicator.title = 'Oldest logs first';
            }
            
            // Sort logs based on selected order
            if (sortOrder === 'newest') {
                allLogs.sort((a, b) => {
                    const timeA = new Date(a.timestamp).getTime();
                    const timeB = new Date(b.timestamp).getTime();
                    return timeB - timeA; // Newest first
                });
            } else {
                allLogs.sort((a, b) => {
                    const timeA = new Date(a.timestamp).getTime();
                    const timeB = new Date(b.timestamp).getTime();
                    return timeA - timeB; // Oldest first
                });
            }
            
            // Re-apply filters and update display
            filterLogs();
        }

        // Navigation functions
        function previousPage() {
            if (currentPage > 1) {
                currentPage--;
                displayLogs();
                updatePagination();
            }
        }
        
        function nextPage() {
            const totalPages = Math.ceil(filteredLogs.length / logsPerPage);
            if (currentPage < totalPages) {
                currentPage++;
                displayLogs();
                updatePagination();
            }
        }

        // Toggle auto-refresh
        function toggleAutoRefresh() {
            const checkbox = document.getElementById('autoRefreshToggle');
            if (checkbox.checked) {
                autoRefreshInterval = setInterval(async () => {
                    // Reload logs
                    const hasNewLogs = await loadLogs();
                    
                    // Show notification for new logs
                    if (hasNewLogs) {
                        showNewLogsNotification();
                    }
                }, 5000);
            } else {
                clearInterval(autoRefreshInterval);
            }
        }

        // Show notification for new logs
        function showNewLogsNotification() {
            let notification = document.getElementById('newLogsNotification');
            if (!notification) {
                notification = document.createElement('div');
                notification.id = 'newLogsNotification';
                notification.className = 'notification';
                document.body.appendChild(notification);
            }
            
            notification.textContent = '🆕 New logs available!';
            notification.style.display = 'block';
            
            setTimeout(() => {
                notification.style.display = 'none';
            }, 3000);
        }

        // Event listeners
        document.getElementById('autoRefreshToggle').addEventListener('change', toggleAutoRefresh);

        // Tab Management
        function showTab(tabName) {
            // Hide all tab contents
            const tabContents = document.querySelectorAll('.tab-content');
            tabContents.forEach(content => content.classList.remove('active'));
            
            // Remove active class from all tab buttons
            const tabButtons = document.querySelectorAll('.tab-button');
            tabButtons.forEach(button => button.classList.remove('active'));
            
            // Show selected tab content
            document.getElementById(tabName + '-tab').classList.add('active');
            
            // Add active class to clicked button
            event.target.classList.add('active');
            
            // Load data for the selected tab
            if (tabName === 'borrowers') {
                loadBorrowers();
            } else if (tabName === 'logs') {
                loadLogs();
            }
        }
        
        // Borrowers Data Management
        let allBorrowers = [];
        let filteredBorrowers = [];
        let currentSortColumn = 'address';
        let currentSortDirection = 'asc';
        
        async function loadBorrowers() {
            try {
                const response = await fetch('/api/borrowers');
                const data = await response.json();
                
                if (data.borrowers) {
                    allBorrowers = Object.values(data.borrowers).map(borrower => ({
                        ...borrower,
                        collateral_count: borrower.collateral ? borrower.collateral.length : 0,
                        debt_count: borrower.debt ? borrower.debt.length : 0,
                        status: getBorrowerStatus(borrower)
                    }));
                    
                    filteredBorrowers = [...allBorrowers];
                    populateFilterDropdowns();
                    renderBorrowersTable();
                    updateBorrowerStats();
                }
            } catch (error) {
                console.error('Error loading borrowers:', error);
                document.getElementById('borrowersTableBody').innerHTML = 
                    '<tr><td colspan="6" class="error">Error loading borrowers data</td></tr>';
            }
        }
        
        function getBorrowerStatus(borrower) {
            const hasCollateral = borrower.collateral && borrower.collateral.length > 0;
            const hasDebt = borrower.debt && borrower.debt.length > 0;
            
            if (hasCollateral && hasDebt) return 'both';
            if (hasCollateral) return 'has_collateral';
            if (hasDebt) return 'has_debt';
            return 'neither';
        }
        
        function populateFilterDropdowns() {
            const collateralTokens = new Set();
            const debtTokens = new Set();
            
            allBorrowers.forEach(borrower => {
                if (borrower.collateral) {
                    borrower.collateral.forEach(token => collateralTokens.add(token));
                }
                if (borrower.debt) {
                    borrower.debt.forEach(token => debtTokens.add(token));
                }
            });
            
            // Populate collateral filter
            const collateralFilter = document.getElementById('collateralFilter');
            collateralFilter.innerHTML = '<option value="">All Collateral</option>';
            Array.from(collateralTokens).sort().forEach(token => {
                const option = document.createElement('option');
                option.value = token;
                option.textContent = token.substring(0, 10) + '...';
                collateralFilter.appendChild(option);
            });
            
            // Populate debt filter
            const debtFilter = document.getElementById('debtFilter');
            debtFilter.innerHTML = '<option value="">All Debt</option>';
            Array.from(debtTokens).sort().forEach(token => {
                const option = document.createElement('option');
                option.value = token;
                option.textContent = token.substring(0, 10) + '...';
                debtFilter.appendChild(option);
            });
        }
        
        function filterBorrowers() {
            const searchTerm = document.getElementById('borrowerSearch').value.toLowerCase();
            const collateralFilter = document.getElementById('collateralFilter').value;
            const debtFilter = document.getElementById('debtFilter').value;
            const statusFilter = document.getElementById('statusFilter').value;
            
            filteredBorrowers = allBorrowers.filter(borrower => {
                // Address search
                if (searchTerm && !borrower.address.toLowerCase().includes(searchTerm)) {
                    return false;
                }
                
                // Collateral filter
                if (collateralFilter && (!borrower.collateral || !borrower.collateral.includes(collateralFilter))) {
                    return false;
                }
                
                // Debt filter
                if (debtFilter && (!borrower.debt || !borrower.debt.includes(debtFilter))) {
                    return false;
                }
                
                // Status filter
                if (statusFilter && borrower.status !== statusFilter) {
                    return false;
                }
                
                return true;
            });
            
            renderBorrowersTable();
            updateBorrowerStats();
        }
        
        function renderBorrowersTable() {
            const tbody = document.getElementById('borrowersTableBody');
            
            if (filteredBorrowers.length === 0) {
                tbody.innerHTML = '<tr><td colspan="6" class="no-data">No borrowers match the current filters</td></tr>';
                return;
            }
            
            tbody.innerHTML = filteredBorrowers.map(borrower => `
                <tr>
                    <td class="address-cell">${borrower.address}</td>
                    <td>${borrower.collateral_count}</td>
                    <td>${borrower.debt_count}</td>
                    <td class="token-list">
                        ${borrower.collateral && borrower.collateral.length > 0 
                            ? borrower.collateral.map(token => 
                                `<span class="token-item" title="${token}">${token.substring(0, 8)}...</span>`
                              ).join('')
                            : '<em>None</em>'
                        }
                    </td>
                    <td class="token-list">
                        ${borrower.debt && borrower.debt.length > 0 
                            ? borrower.debt.map(token => 
                                `<span class="token-item" title="${token}">${token.substring(0, 8)}...</span>`
                              ).join('')
                            : '<em>None</em>'
                        }
                    </td>
                    <td>
                        <span class="status-badge status-${borrower.status}">
                            ${getStatusDisplayName(borrower.status)}
                        </span>
                    </td>
                </tr>
            `).join('');
        }
        
        function getStatusDisplayName(status) {
            switch (status) {
                case 'has_collateral': return 'Collateral Only';
                case 'has_debt': return 'Debt Only';
                case 'both': return 'Both';
                case 'neither': return 'Neither';
                default: return 'Unknown';
            }
        }
        
        function updateBorrowerStats() {
            const total = filteredBorrowers.length;
            const hasCollateral = filteredBorrowers.filter(b => b.collateral_count > 0).length;
            const hasDebt = filteredBorrowers.filter(b => b.debt_count > 0).length;
            const both = filteredBorrowers.filter(b => b.status === 'both').length;
            const neither = filteredBorrowers.filter(b => b.status === 'neither').length;
            
            document.getElementById('borrowerStats').innerHTML = 
                `Showing ${total} borrowers | ${hasCollateral} with collateral | ${hasDebt} with debt | ${both} with both | ${neither} with neither`;
        }
        
        function sortTable(column) {
            if (currentSortColumn === column) {
                currentSortDirection = currentSortDirection === 'asc' ? 'desc' : 'asc';
            } else {
                currentSortColumn = column;
                currentSortDirection = 'asc';
            }
            
            filteredBorrowers.sort((a, b) => {
                let aVal = a[column];
                let bVal = b[column];
                
                if (column === 'address') {
                    aVal = aVal.toLowerCase();
                    bVal = bVal.toLowerCase();
                }
                
                if (aVal < bVal) return currentSortDirection === 'asc' ? -1 : 1;
                if (aVal > bVal) return currentSortDirection === 'asc' ? 1 : -1;
                return 0;
            });
            
            renderBorrowersTable();
        }
        
        // Initialize
        document.addEventListener('DOMContentLoaded', () => {
            loadLogFiles();
            loadLogs();
            toggleAutoRefresh(); // Start auto-refresh
            
            // Load borrowers data if borrowers tab is active
            if (document.querySelector('#borrowers-tab.active')) {
                loadBorrowers();
            }
        });
    </script>
</body>
</html>
"""

def parse_log_line(line):
    """Parse a log line and extract structured information"""
    line = line.strip()
    if not line:
        return None
    
    # Parse timestamp and log level
    timestamp_match = re.match(r'^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z)', line)
    if not timestamp_match:
        return None
    
    timestamp = timestamp_match.group(1)
    remaining = line[len(timestamp):].strip()
    
    # Parse log level (handle variable spacing)
    level_match = re.match(r'^\s*(\w+)', remaining)
    if not level_match:
        return None
    
    level = level_match.group(1)
    remaining = remaining[len(level_match.group(0)):].strip()
    
    # Parse thread information (handle variable spacing)
    thread_match = re.match(r'^(\w+)\s+ThreadId\((\d+)\)', remaining)
    if thread_match:
        thread_name = thread_match.group(1)
        thread_id = f"ThreadId({thread_match.group(2)})"
        thread = f"{thread_name} {thread_id}"
        remaining = remaining[len(thread_match.group(0)):].strip()
    else:
        # Try to find any thread-like pattern
        thread_pattern = re.search(r'ThreadId\(\d+\)', remaining)
        if thread_pattern:
            thread = thread_pattern.group(0)
            remaining = remaining[:thread_pattern.start()] + remaining[thread_pattern.end():]
            remaining = remaining.strip()
        else:
            thread = "Unknown"
    
    # For module and message, handle double colons and extract actual message
    colon_pos = remaining.find(':')
    if colon_pos > 0:
        module = remaining[:colon_pos].strip()
        message_part = remaining[colon_pos + 1:].strip()
        
        # If the message part starts with another colon (like ::strategies::aave_strategy:)
        # then the actual message is after the last colon
        if message_part.startswith(':'):
            # Find the last colon in the message part
            last_colon = message_part.rfind(':')
            if last_colon > 0:
                # Everything after the last colon is the actual message
                message = message_part[last_colon + 1:].strip()
            else:
                message = message_part
        else:
            message = message_part
    else:
        module = remaining
        message = ""
    
    return {
        'timestamp': timestamp,
        'level': level,
        'thread': thread,
        'module': module,
        'message': message,
        'raw': line
    }

def get_log_files():
    """Get list of available log files"""
    log_dir = '.botdata/logs'
    if not os.path.exists(log_dir):
        return []
    
    log_files = glob.glob(os.path.join(log_dir, '*.log'))
    return sorted(log_files, reverse=True)  # Most recent first

def read_logs(log_file=None):
    """Read and parse logs from file(s)"""
    logs = []
    stats = {'total': 0, 'errors': 0, 'info': 0, 'files': 0}
    
    if log_file:
        files_to_read = [log_file] if os.path.exists(log_file) else []
    else:
        files_to_read = get_log_files()
    
    stats['files'] = len(files_to_read)
    
    print(f"📁 Reading {len(files_to_read)} log file(s)...")
    
    for file_path in files_to_read:
        try:
            print(f"📖 Reading: {file_path}")
            with open(file_path, 'r', encoding='utf-8') as f:
                lines_read = 0
                lines_parsed = 0
                for line_num, line in enumerate(f, 1):
                    lines_read += 1
                    if line.strip():
                        parsed = parse_log_line(line)
                        if parsed:
                            logs.append(parsed)
                            lines_parsed += 1
                            stats['total'] += 1
                            
                            if parsed['level'] == 'ERROR':
                                stats['errors'] += 1
                            elif parsed['level'] == 'INFO':
                                stats['info'] += 1
                
                print(f"   ✅ Read {lines_read} lines, parsed {lines_parsed} successfully")
                
        except Exception as e:
            print(f"❌ Error reading {file_path}: {e}")
    
    print(f"📊 Total logs parsed: {stats['total']}")
    return logs, stats

@app.route('/')
def index():
    return HTML_TEMPLATE

@app.route('/api/log-files')
def api_log_files():
    return jsonify(get_log_files())

@app.route('/api/logs')
def api_logs():
    log_file = request.args.get('file', '')
    if log_file and log_file.startswith('.botdata/logs/'):
        # Use the full path as provided
        logs, stats = read_logs(log_file)
    else:
        # Read all logs
        logs, stats = read_logs()
    return jsonify({'logs': logs, 'stats': stats})

# Add new API endpoint for borrowers data from SQLite
@app.route('/api/borrowers')
def api_borrowers():
    try:
        # Try to connect to SQLite database
        db_path = '.botdata/borrowers.db'
        if not os.path.exists(db_path):
            return jsonify({'error': 'SQLite database not found', 'path': db_path}), 404
        
        conn = sqlite3.connect(db_path)
        cursor = conn.cursor()
        
        # Get last processed block number
        cursor.execute("SELECT value FROM bot_state WHERE key = 'last_block_number'")
        block_result = cursor.fetchone()
        last_block_number = int(block_result[0]) if block_result else 0
        
        # Get all borrowers with their collateral and debt counts
        cursor.execute("""
            SELECT address, collateral_count, debt_count, created_at, updated_at 
            FROM borrowers 
            ORDER BY updated_at DESC
        """)
        borrowers_data = cursor.fetchall()
        
        # Get detailed asset information for each borrower
        borrowers = []
        for borrower_row in borrowers_data:
            address, collateral_count, debt_count, created_at, updated_at = borrower_row
            
            # Get collateral assets
            cursor.execute("""
                SELECT asset_address FROM borrower_collateral 
                WHERE borrower_address = ?
            """, (address,))
            collateral_assets = [row[0] for row in cursor.fetchall()]
            
            # Get debt assets
            cursor.execute("""
                SELECT asset_address FROM borrower_debt 
                WHERE borrower_address = ?
            """, (address,))
            debt_assets = [row[0] for row in cursor.fetchall()]
            
            # Determine status
            if collateral_count > 0 and debt_count > 0:
                status = 'both'
            elif collateral_count > 0:
                status = 'collateral_only'
            elif debt_count > 0:
                status = 'debt_only'
            else:
                status = 'neither'
            
            borrowers.append({
                'address': address,
                'collateral_count': collateral_count,
                'debt_count': debt_count,
                'collateral_assets': collateral_assets,
                'debt_assets': debt_assets,
                'status': status,
                'created_at': created_at,
                'updated_at': updated_at
            })
        
        conn.close()
        
        data = {
            'last_block_number': last_block_number,
            'borrowers': borrowers
        }
        
        return jsonify(data)
        
    except Exception as e:
        return jsonify({'error': str(e)}), 500

if __name__ == '__main__':
    print("🚀 Starting Artemis Log Viewer...")
    print("📱 Open your browser and go to: http://localhost:5000")
    print("📁 Looking for logs in: .botdata/logs/")
    print("⏹️  Press Ctrl+C to stop")
    print()
    
    app.run(host='0.0.0.0', port=5000, debug=False)
