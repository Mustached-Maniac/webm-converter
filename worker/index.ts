import { Container, getRandom, getContainer } from '@cloudflare/containers';

export class WebmConverter extends Container {
  defaultPort = 8666;
  sleepAfter = '5m';
  
  override onStart() {
    console.log('Container successfully started');
  }
  
  override onStop() {
    console.log('Container successfully shut down');
  }
  
  override onError(error: unknown) {
    console.log('Container error:', error);
  }
}

interface Env {
  WEBM_CONVERTER: any;
}

async function getHealthyContainer(env: Env, maxRetries = 3): Promise<any> {
  for (let attempt = 0; attempt < maxRetries; attempt++) {
    try {
      const container = await getRandom(env.WEBM_CONVERTER);
      
      const healthCheck = await Promise.race([
        container.fetch(new Request('http://container/health')),
        new Promise((_, reject) => 
          setTimeout(() => reject(new Error('Health check timeout')), 2000)
        )
      ]) as Response;
      
      if (healthCheck.ok) {
        console.log(`Container healthy on attempt ${attempt + 1}`);
        return container;
      }
    } catch (e) {
      console.log(`Attempt ${attempt + 1} failed:`, e);
      if (attempt < maxRetries - 1) {
        await new Promise(resolve => setTimeout(resolve, 1000));
      }
    }
  }
  
  throw new Error('No healthy container available');
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    
    if (url.pathname === '/health') {
      return new Response(JSON.stringify({ status: 'ok' }), {
        headers: { 'Content-Type': 'application/json' }
      });
    }
    
    console.log('=== WORKER REQUEST ===');
    console.log('Method:', request.method, 'Path:', url.pathname);
    
    if (url.pathname === '/upload' && request.method === 'POST') {
      try {
        const jobId = crypto.randomUUID();
        console.log(`Generated job_id: ${jobId}`);
        const container = getContainer(env.WEBM_CONVERTER, jobId);
        console.log(`Using container with affinity for job: ${jobId}`);
        const clonedRequest = new Request(request.url, {
          method: request.method,
          headers: {
            ...Object.fromEntries(request.headers),
            'X-Job-Id': jobId
          },
          body: request.body,
          duplex: 'half'
        } as any);
        
        const uploadPromise = container.fetch(clonedRequest);
        const timeoutPromise = new Promise<Response>((_, reject) =>
          setTimeout(() => reject(new Error('Upload timeout')), 25000)
        );
        
        const response = await Promise.race([
          uploadPromise,
          timeoutPromise
        ]) as Response;
        
        console.log('Upload complete:', response.status);
        return response;
        
      } catch (error) {
        console.error('Upload error:', error);
        
        const errorMessage = error instanceof Error ? error.message : String(error);
        const isTimeout = errorMessage.includes('timeout');
        
        return new Response(JSON.stringify({ 
          error: isTimeout ? 'Upload timeout - file too large' : 'Upload failed',
          details: errorMessage,
        }), { 
          status: isTimeout ? 504 : 500,
          headers: { 'Content-Type': 'application/json' }
        });
      }
    }
    
    if (url.pathname.startsWith('/status/') && request.method === 'GET') {
      try {
        const jobId = url.pathname.split('/status/')[1];
        const container = getContainer(env.WEBM_CONVERTER, jobId);
        console.log(`Using container with affinity for job: ${jobId}`);
        const statusPromise = container.fetch(request);
        const timeoutPromise = new Promise<Response>((_, reject) =>
          setTimeout(() => reject(new Error('Status check timeout')), 5000)
        );
        
        const response = await Promise.race([
          statusPromise,
          timeoutPromise
        ]) as Response;
        
        return response;
        
      } catch (error) {
        console.error('Status check error:', error);
        return new Response(JSON.stringify({ 
          error: 'Failed to check status',
          details: String(error)
        }), { 
          status: 500,
          headers: { 'Content-Type': 'application/json' }
        });
      }
    }
    
    if (url.pathname.startsWith('/download/') && request.method === 'GET') {
      try {
        const jobId = url.pathname.split('/download/')[1];
        const container = getContainer(env.WEBM_CONVERTER, jobId);
        console.log(`Using container with affinity for job: ${jobId}`);
        const downloadPromise = container.fetch(request);
        const timeoutPromise = new Promise<Response>((_, reject) =>
          setTimeout(() => reject(new Error('Download timeout')), 30000)
        );
        
        const response = await Promise.race([
          downloadPromise,
          timeoutPromise
        ]) as Response;
        
        console.log('Download complete:', response.status);
        return response;
        
      } catch (error) {
        console.error('Download error:', error);
        return new Response(JSON.stringify({ 
          error: 'Failed to download video',
          details: String(error)
        }), { 
          status: 500,
          headers: { 'Content-Type': 'application/json' }
        });
      }
    }
    
    return new Response(JSON.stringify({ 
      error: 'Not found',
      available_endpoints: [
        'POST /upload',
        'GET /status/{job_id}',
        'GET /download/{job_id}'
      ]
    }), { 
      status: 404,
      headers: { 'Content-Type': 'application/json' }
    });
  },
};
