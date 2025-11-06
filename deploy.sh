#!/bin/bash

# Video Converter - Quick Deploy Script
# This script automates the deployment of your video converter to Cloudflare

set -e  # Exit on error

echo "🚀 Video Converter Deployment Script"
echo "===================================="
echo ""

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if Docker is running
echo "Checking Docker..."
if ! docker info > /dev/null 2>&1; then
    echo -e "${RED} Error: Docker is not running${NC}"
    echo "Please start Docker Desktop and try again"
    exit 1
fi
echo -e "${GREEN} Docker is running${NC}"
echo ""

# Check if Node.js is installed
echo "Checking Node.js..."
if ! command -v node &> /dev/null; then
    echo -e "${RED} Error: Node.js is not installed${NC}"
    echo "Please install Node.js from https://nodejs.org/"
    exit 1
fi
echo -e "${GREEN} Node.js $(node --version) found${NC}"
echo ""

# Check if wrangler is installed globally, if not install locally
echo "Checking Wrangler CLI..."
if ! command -v wrangler &> /dev/null; then
    echo -e "${YELLOW}  Wrangler not found globally, will use npx${NC}"
    WRANGLER="npx wrangler"
else
    echo -e "${GREEN} Wrangler CLI found${NC}"
    WRANGLER="wrangler"
fi
echo ""

# Install npm dependencies
echo "Installing dependencies..."
npm install
echo -e "${GREEN} Dependencies installed${NC}"
echo ""

# Check if user is logged in to Wrangler
echo "Checking Cloudflare authentication..."
if ! $WRANGLER whoami > /dev/null 2>&1; then
    echo -e "${YELLOW}  Not logged in to Cloudflare${NC}"
    echo "Opening browser for authentication..."
    $WRANGLER login
else
    echo -e "${GREEN} Already authenticated${NC}"
fi
echo ""

# Optional: Build and test container locally first
echo "Would you like to test the container locally first? (y/n)"
read -r -p "This will build the Docker image and test it: " TEST_LOCAL

if [[ $TEST_LOCAL =~ ^[Yy]$ ]]; then
    echo ""
    echo "Building Docker image..."
    docker build -t webm-converter .
    
    echo ""
    echo "Testing container..."
    echo "Starting container on port 8666..."
    
    # Run container in background
    CONTAINER_ID=$(docker run -d -p 8666:8666 webm-converter)
    
    # Wait for container to start
    sleep 5
    
    # Test health endpoint
    if curl -s http://localhost:8666/health > /dev/null; then
        echo -e "${GREEN} Container is running and healthy!${NC}"
        echo ""
        echo "Container is running at: http://localhost:8666"
        echo "Container ID: $CONTAINER_ID"
        echo ""
        echo "You can test conversion with:"
        echo "  curl -X POST -F \"file=@test-video.mp4\" http://localhost:8666/convert --output test.webm"
        echo ""
        read -r -p "Press Enter to stop the container and continue with deployment..."
        docker stop $CONTAINER_ID > /dev/null
        docker rm $CONTAINER_ID > /dev/null
        echo -e "${GREEN} Test container stopped${NC}"
    else
        echo -e "${RED} Container health check failed${NC}"
        docker stop $CONTAINER_ID > /dev/null
        docker rm $CONTAINER_ID > /dev/null
        exit 1
    fi
    echo ""
fi

# Deploy to Cloudflare
echo "Deploying to Cloudflare..."
echo -e "${YELLOW} This may take a few minutes (building and pushing Docker image)...${NC}"
echo ""

if $WRANGLER deploy; then
    echo ""
    echo -e "${GREEN} Deployment successful!${NC}"
    echo ""
    echo "=========================================="
    echo "🎉 DEPLOYMENT COMPLETE!"
    echo "=========================================="
    echo ""
    
    # Get worker URL
    echo "Your worker is deployed. Getting URL..."
    echo ""
    
    echo -e "${YELLOW}  IMPORTANT: Wait 5-10 minutes for containers to provision${NC}"
    echo ""
    echo "Next steps:"
    echo "1. Check container status:"
    echo "   $WRANGLER containers list"
    echo ""
    echo "2. View logs:"
    echo "   $WRANGLER tail"
    echo ""
    echo "3. Add to your .env file:"
    echo "   CLOUDFLARE_CONVERTER_URL=<your-worker-url>"
    echo ""
    echo "4. Monitor in dashboard:"
    echo "   https://dash.cloudflare.com/?to=/:account/workers/containers"
    echo ""
    
    # Optional: Check container status immediately
    echo "Checking container status..."
    $WRANGLER containers list
    echo ""
    
    echo -e "${GREEN} All done! Your video converter is deployed.${NC}"
    echo ""
    echo "See DEPLOYMENT.md and README.md for more information"
    
else
    echo ""
    echo -e "${RED} Deployment failed${NC}"
    echo "Please check the error messages above"
    echo "Common issues:"
    echo "  - Docker not running"
    echo "  - Not logged in to Cloudflare (run: $WRANGLER login)"
    echo "  - Insufficient permissions"
    exit 1
fi
