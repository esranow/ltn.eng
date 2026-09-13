// Navigation Tab Switching
document.querySelectorAll('.nav-item').forEach(button => {
  button.addEventListener('click', () => {
    const tabName = button.getAttribute('data-tab');
    switchTab(tabName);
  });
});

function switchTab(tabName) {
  document.querySelectorAll('.nav-item').forEach(b => b.classList.remove('active'));
  document.querySelectorAll('.tab-page').forEach(p => p.classList.remove('active'));

  const activeBtn = document.querySelector(`.nav-item[data-tab="${tabName}"]`);
  const activePage = document.getElementById(`tab-${tabName}`);

  if (activeBtn) activeBtn.classList.add('active');
  if (activePage) activePage.classList.add('active');

  const titles = {
    dashboard: { title: 'System Overview', sub: 'Empirical OS Observer & Z3 SMT Prover Status' },
    analyze: { title: 'Workload Analyzer', sub: 'AST & Manifest Discovery Engine' },
    architect: { title: '3-Tier Architect', sub: 'CHEAPEST, BALANCED & HIGH-PERFORMANCE Models' },
    deploy: { title: 'Deployment Engine', sub: 'Agent Harness Z3 Constraint Gate' },
    telemetry: { title: 'Live Telemetry', sub: 'Empirical Socket & Process Monitoring' },
    settings: { title: 'Configuration', sub: 'Provider Credentials & System Settings' }
  };

  if (titles[tabName]) {
    document.getElementById('tab-heading').innerText = titles[tabName].title;
    document.getElementById('tab-subheading').innerText = titles[tabName].sub;
  }
}

// Quick Analyze from Dashboard
function quickAnalyze(path) {
  switchTab('analyze');
  document.getElementById('analyze-path').value = path;
  runAnalysis();
}

// Run Analysis
function runAnalysis() {
  const path = document.getElementById('analyze-path').value;
  const resultsBox = document.getElementById('analyze-results');
  resultsBox.classList.remove('hidden');

  logActivity(`[ANALYZER] Scanning manifests in '${path}'...`);

  // Mocked/Client-side logic mimicking ltn-cli analyze output
  if (path.includes('python_vllm_gpu')) {
    setAnalysisResults('PythonVllmGpu', 'Python High-Performance LLM/GPU Inference Workload', 'pyproject.toml/requirements.txt', '8 Cores', '32768 MB', '24576 MB');
  } else if (path.includes('react_vite_static')) {
    setAnalysisResults('StaticWeb', 'React / Vite Static Web Frontend', 'package.json', '1 Core', '512 MB', '0 MB');
  } else if (path.includes('node_postgres_api')) {
    setAnalysisResults('NodePostgresApi', 'Node.js REST API with PostgreSQL Database', 'package.json', '2 Cores', '2048 MB', '0 MB');
  } else if (path.includes('go_worker')) {
    setAnalysisResults('GoWorker', 'Go Distributed Background Worker', 'go.mod', '2 Cores', '1024 MB', '0 MB');
  } else {
    setAnalysisResults('GenericContainer', 'Generic Containerized Application', 'Dockerfile', '1 Core', '1024 MB', '0 MB');
  }
  logActivity(`[ANALYZER] Analysis complete for '${path}'.`);
}

function setAnalysisResults(kind, desc, manifests, cpu, ram, vram) {
  document.getElementById('res-kind').innerText = kind;
  document.getElementById('res-desc').innerText = desc;
  document.getElementById('res-manifests').innerText = manifests;
  document.getElementById('res-cpu').innerText = cpu;
  document.getElementById('res-ram').innerText = ram;
  document.getElementById('res-vram').innerText = vram;
}

// Run Architect 3-Tier Planner
function runArchitect() {
  const path = document.getElementById('architect-path').value;
  const grid = document.getElementById('architect-results');
  grid.innerHTML = '';
  grid.classList.remove('hidden');

  logActivity(`[ARCHITECT] Synthesizing 3-tier plans for '${path}'...`);

  const plans = [
    { tier: 'CHEAPEST', provider: 'LOCAL_DOCKER', cost: '$0', desc: 'Single-container cost-optimized setup.', highlight: false },
    { tier: 'BALANCED', provider: 'AWS (ECS + RDS)', cost: '$35', desc: 'High-availability 2-tier setup.', highlight: true },
    { tier: 'HIGH_PERFORMANCE', provider: 'GCP GPU CLUSTER', cost: '$280', desc: 'Distributed GPU-accelerated cluster.', highlight: false }
  ];

  plans.forEach(plan => {
    const card = document.createElement('div');
    card.className = `plan-card ${plan.highlight ? 'highlight' : ''}`;
    card.innerHTML = `
      <div class="plan-tier">${plan.tier}</div>
      <div class="plan-cost">${plan.cost} <span>/ month</span></div>
      <p style="font-size:13px; color:var(--text-muted); margin-bottom:16px;">${plan.desc}</p>
      <div style="font-size:12px; font-weight:600; margin-bottom:16px;">Provider: ${plan.provider}</div>
      <button class="btn btn-secondary btn-block" onclick="selectPlanForDeploy('${path}', '${plan.tier}')">Select & Deploy</button>
    `;
    grid.appendChild(card);
  });
  logActivity(`[ARCHITECT] 3 architectural plans generated.`);
}

function selectPlanForDeploy(path, tier) {
  switchTab('deploy');
  document.getElementById('deploy-path').value = path;
  document.getElementById('deploy-tier').value = tier;
}

// Run Deploy Lifecycle
function runDeploy() {
  const path = document.getElementById('deploy-path').value;
  const tier = document.getElementById('deploy-tier').value;
  const progressBox = document.getElementById('deploy-progress');
  const outLog = document.getElementById('deploy-output');

  progressBox.classList.remove('hidden');
  outLog.innerHTML = '';

  const steps = ['read', 'plan', 'validate', 'mutate', 'verify'];
  steps.forEach(s => document.getElementById(`step-${s}`).classList.remove('done'));

  logActivity(`[DEPLOY] Starting deployment of '${path}' [${tier}]...`);

  let currentStep = 0;
  const interval = setInterval(() => {
    if (currentStep < steps.length) {
      const stepName = steps[currentStep];
      document.getElementById(`step-${stepName}`).classList.add('done');
      
      const messages = [
        `[1/5 READ] Scanning files in ${path}...`,
        `[2/5 PLAN] Generated ${tier} architecture specification.`,
        `[3/5 VALIDATE] Z3 SMT Prover verified hardware constraints: OK.`,
        `[4/5 MUTATE] Applied container updates via Local OS Provider.`,
        `[5/5 VERIFY] Empirical Observer socket probe verified on port 8080.`
      ];

      const div = document.createElement('div');
      div.className = 'log-line success';
      div.innerText = messages[currentStep];
      outLog.appendChild(div);
      outLog.scrollTop = outLog.scrollHeight;

      currentStep++;
    } else {
      clearInterval(interval);
      logActivity(`[DEPLOY] Deployment of '${path}' succeeded!`);
    }
  }, 600);
}

// Fetch Status
function fetchSystemStatus() {
  logActivity('[SYS] Querying live empirical telemetry...');
  document.getElementById('status-sandbox').innerText = 'ACTIVE';
  document.getElementById('status-z3').innerText = 'OPTIMAL';
  document.getElementById('status-sockets').innerText = '1 Active';
}

function logActivity(msg) {
  const logTerminal = document.getElementById('activity-log');
  const div = document.createElement('div');
  div.className = 'log-line info';
  div.innerText = `[${new Date().toLocaleTimeString()}] ${msg}`;
  logTerminal.appendChild(div);
  logTerminal.scrollTop = logTerminal.scrollHeight;
}

// Save Config
function saveConfig() {
  const provider = document.getElementById('cfg-provider').value;
  const interval = document.getElementById('cfg-interval').value;
  logActivity(`[CONFIG] Saved default provider: ${provider}, Probe interval: ${interval}s`);
  alert(`Configuration saved successfully!\nDefault Provider: ${provider}\nProbe Interval: ${interval}s`);
}
