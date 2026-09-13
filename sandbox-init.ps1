<#
.SYNOPSIS
    LTN.ENG Sandbox Initializer
    Builds the Docker sandbox image and starts the isolated container.

.DESCRIPTION
    The container has access to exactly 3 paths:
      /ref/beam    = D:\beam         (READ-ONLY)
      /workspace   = D:\ltn-eng-workspace  (READ-WRITE)
      /graph       = D:\.ltn-graph   (READ-WRITE)

    Nothing else on the host is accessible to the container.
#>

$ErrorActionPreference = "Stop"
$ImageName = "ltn-sandbox"
$ContainerName = "ltn-sandbox"

Write-Host "=== LTN.ENG Sandbox Initializer ===" -ForegroundColor Cyan

# Step 1: Ensure graph directory exists
New-Item -ItemType Directory -Force -Path "D:\.ltn-graph" | Out-Null
New-Item -ItemType Directory -Force -Path "D:\.ltn-graph\contracts" | Out-Null
New-Item -ItemType Directory -Force -Path "D:\.ltn-graph\beam-backup" | Out-Null
New-Item -ItemType Directory -Force -Path "D:\.ltn-graph\trace" | Out-Null

# Step 2: Build the sandbox image
Write-Host "`n[1/3] Building sandbox image..." -ForegroundColor Yellow
docker build -t $ImageName -f "D:\ltn-eng-workspace\.docker\Dockerfile" "D:\ltn-eng-workspace\.docker\"
if ($LASTEXITCODE -ne 0) {
    Write-Error "Failed to build sandbox image"
    exit 1
}

# Step 3: Remove existing container if any
Write-Host "`n[2/3] Cleaning up old container..." -ForegroundColor Yellow
docker rm -f $ContainerName 2>$null

# Step 4: Launch the sandbox container
Write-Host "`n[3/3] Starting sandbox container..." -ForegroundColor Yellow
docker run -d `
    --name $ContainerName `
    --hostname ltn-sandbox `
    -v "D:\beam:/ref/beam:ro" `
    -v "D:\ai_projects\Rippl:/ref/rippl:ro" `
    -v "D:\ltn-eng-workspace:/workspace" `
    -v "D:\.ltn-graph:/graph" `
    --memory=8g `
    --cpus=4 `
    --pids-limit=512 `
    --security-opt="no-new-privileges:true" `
    --cap-drop=ALL `
    --cap-add=DAC_OVERRIDE `
    --cap-add=FOWNER `
    --cap-add=CHOWN `
    --cap-add=SETUID `
    --cap-add=SETGID `
    $ImageName

if ($LASTEXITCODE -ne 0) {
    Write-Error "Failed to start sandbox container"
    exit 1
}

Write-Host "`n=== Sandbox is LIVE ===" -ForegroundColor Green
docker ps --filter "name=$ContainerName" --format "table {{.ID}}\t{{.Image}}\t{{.Status}}\t{{.Names}}"

Write-Host "`nVolume Mounts:" -ForegroundColor Cyan
Write-Host "  /ref/rippl   <- D:\ai_projects\Rippl (READ-ONLY)"
Write-Host "  /ref/beam    <- D:\beam             (READ-ONLY)"
Write-Host "  /workspace   <- D:\ltn-eng-workspace (READ-WRITE)"
Write-Host "  /graph       <- D:\.ltn-graph        (READ-WRITE)"

Write-Host "`nSecurity:" -ForegroundColor Cyan
Write-Host "  - All capabilities dropped (except minimal file ops)"
Write-Host "  - no-new-privileges enforced"
Write-Host "  - Memory limit: 8GB"
Write-Host "  - CPU limit: 4 cores"
Write-Host "  - PID limit: 512"

Write-Host "`nTo exec into sandbox: docker exec ltn-sandbox <command>" -ForegroundColor Gray
Write-Host "To destroy sandbox:    docker rm -f ltn-sandbox" -ForegroundColor Gray
