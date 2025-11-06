# Video to WebM Converter - Cloudflare Containers

A high-performance async video conversion service using Cloudflare Workers and Containers with FFmpeg. Converts videos to optimized WebM format at the edge with job-based processing.

## Architecture

```
User → Nuxt App → Cloudflare Worker → Container (Rust + FFmpeg)
                                            ↓
                        Upload → Background Processing → Download
```

1. **Cloudflare Worker**: Routes requests and manages container lifecycle
2. **Rust Server**: Multi-threaded Actix-Web server with async job processing
3. **FFmpeg**: Converts videos using VP9 codec with optimization
4. **Job System**: Upload returns immediately, conversion happens in background

## Prerequisites

- [Docker Desktop](https://www.docker.com/products/docker-desktop/) (running)
- [Node.js](https://nodejs.org/) v18+
- [Wrangler CLI](https://developers.cloudflare.com/workers/wrangler/)
- Cloudflare account with Containers access

## Installation

```bash
# Clone and navigate
cd webm-converter

# Install dependencies
npm install

# Login to Cloudflare
npx wrangler login

# Deploy (Docker must be running)
npx wrangler deploy
```

## API Endpoints

### POST /upload
Upload a video and receive a job ID for tracking.

**Request:**
```bash
curl -X POST -F "file=@video.mp4" \
  -F "crf=30" \
  -F "audio_bitrate=128k" \
  -F "detect_green=true" \
  https://your-worker.workers.dev/upload
```

**Response:**
```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "processing"
}
```

### GET /status/{job_id}
Check the conversion progress.

**Request:**
```bash
curl https://your-worker.workers.dev/status/550e8400-e29b-41d4-a716-446655440000
```

**Response:**
```json
{
  "status": "processing",
  "progress": 65,
  "detected_green": "0x0BB600"
}
```

**Status values:**
- `processing` - Conversion in progress
- `complete` - Ready to download
- `failed` - Conversion failed

**Progress values:** 0-100

### GET /download/{job_id}
Download the converted video (only when status is `complete`).

**Request:**
```bash
curl https://your-worker.workers.dev/download/550e8400-e29b-41d4-a716-446655440000 \
  --output converted.webm
```

**Response:**
- Content-Type: `video/webm`
- Header: `X-Detected-Green` (if green detection was enabled)
- Body: WebM video file

## Configuration

### Container Settings (`wrangler.toml`)

```toml
[[containers]]
class_name = "WebmConverter"
image = "./Dockerfile"
max_instances = 3
instance_type = "standard-2"  # 1 vCPU, 6 GiB RAM
```

**Available instance types:**
- `lite`: 1/16 vCPU, 256 MiB (not recommended for video)
- `basic`: 1/4 vCPU, 1 GiB
- `standard-1`: 1/2 vCPU, 4 GiB
- `standard-2`: 1 vCPU, 6 GiB **Recommended**
- `standard-3`: 2 vCPU, 8 GiB
- `standard-4`: 4 vCPU, 12 GiB

### Conversion Options

**CRF (Constant Rate Factor):** 0-63 (default: 30)
- Lower = better quality, larger file
- Higher = lower quality, smaller file

**Audio Bitrate:** (default: 128k)
- Common values: 64k, 96k, 128k, 192k, 256k

**Detect Green:** true/false (default: false)
- Samples corner pixels to detect green screen color
- Returns hex color value in response header

## Local Development

### Option 1: Full Stack Local

```bash
# Terminal 1: Build and run container
docker build -t webm-converter .
docker run -p 8666:8666 webm-converter

# Terminal 2: Run worker locally
npx wrangler dev

# Test at http://localhost:8787
```

### Option 2: Container Only

```bash
# Run container
docker run -p 8666:8666 webm-converter

# Test upload
curl -X POST -F "file=@test.mp4" \
  http://localhost:8666/upload

# Check status
curl http://localhost:8666/status/{job_id}

# Download result
curl http://localhost:8666/download/{job_id} --output result.webm
```

## Monitoring & Debugging

```bash
# View live logs
npx wrangler tail

# List running containers
npx wrangler containers list

# List container images
npx wrangler containers images list

# Check container health
curl https://your-worker.workers.dev/health
```

## Client Usage Example

```typescript
// Upload video
async function uploadVideo(file: File) {
  const formData = new FormData();
  formData.append('file', file);
  formData.append('crf', '30');
  formData.append('detect_green', 'true');
  
  const response = await fetch('/upload', {
    method: 'POST',
    body: formData
  });
  
  const { job_id } = await response.json();
  return job_id;
}

// Poll for completion
async function waitForCompletion(jobId: string) {
  while (true) {
    const response = await fetch(`/status/${jobId}`);
    const { status, progress } = await response.json();
    
    console.log(`Progress: ${progress}%`);
    
    if (status === 'complete') {
      return true;
    }
    if (status === 'failed') {
      throw new Error('Conversion failed');
    }
    
    await new Promise(resolve => setTimeout(resolve, 1000));
  }
}

// Download result
async function downloadVideo(jobId: string) {
  const response = await fetch(`/download/${jobId}`);
  const blob = await response.blob();
  const greenColor = response.headers.get('X-Detected-Green');
  
  return { blob, greenColor };
}

// Complete flow
async function convertVideo(file: File) {
  const jobId = await uploadVideo(file);
  await waitForCompletion(jobId);
  return await downloadVideo(jobId);
}
```

## Output Specifications

- **Container**: WebM
- **Video Codec**: VP9
- **Audio Codec**: Opus (128k default)
- **Optimization**: Row-based multithreading, 2 threads
- **Compatibility**: Modern browsers (Chrome, Firefox, Edge, Safari 14.1+)

## Performance Characteristics

- **Upload time**: ~15-20 seconds (depends on file size and network)
- **Conversion time**: 20-40 seconds (depends on video length and complexity)
- **Cold start**: 10-30 seconds for first request
- **Warm processing**: Subsequent requests use warm containers
- **Max concurrent jobs**: Limited by `max_instances` setting (default: 3)
- **Recommended max file size**: 100MB

## Supported Input Formats

- MP4 (H.264, H.265)
- MOV
- AVI
- MKV
- WebM
- GIF
- Most formats supported by FFmpeg

## FFmpeg Quality Settings (CRF)

| CRF Value | Quality    | File Size  | Use Case               |
|-----------|------------|------------|------------------------|
| 15-20     | Excellent  | Large      | High-quality archives  |
| 23-28     | Very Good  | Medium     | General use            |
| 31-35     | Good       | Small      | Web streaming          |
| 35+       | Lower      | Very Small | Low bandwidth          |

**Default: CRF 30** - Good balance of quality and file size for web use

## Troubleshooting

### Upload Timeout
- **Cause**: File too large or slow network
- **Solution**: Reduce file size or increase timeout in worker

### Conversion Failed
- **Cause**: Unsupported format or corrupted file
- **Solution**: Check file format, try re-encoding input

### Container Not Starting
- **Cause**: Docker not running or resource limits
- **Solution**: Ensure Docker Desktop is running, check `wrangler.toml` settings

### Job Stuck at Processing
- **Cause**: Video too complex for instance type
- **Solution**: Upgrade to `standard-3` or `standard-4` instance type

## Cost Optimization

- Use appropriate `instance_type` for your workload
- Set `max_instances` based on expected concurrent load
- Jobs complete faster with more CPU, often reducing total cost
- Monitor container usage in Cloudflare dashboard

## Limitations

- Worker CPU time: 60 seconds max per request (upload/download)
- Container processing: No hard limit (async background jobs)
- File storage: Temporary (cleaned up after download)
- Concurrent jobs: Limited by `max_instances` setting