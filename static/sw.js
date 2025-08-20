// Service Worker for WebRTC Video Chat PWA
const CACHE_NAME = 'webrtc-video-chat-v2.0';
const STATIC_CACHE = 'static-v2.0';
const DYNAMIC_CACHE = 'dynamic-v2.0';

const urlsToCache = [
  '/',
  '/login.html',
  '/admin.html',
  '/static/manifest.json',
  'https://via.placeholder.com/192x192/667eea/ffffff?text=VC',
  'https://via.placeholder.com/512x512/667eea/ffffff?text=VC'
];

const NETWORK_TIMEOUT = 3000;

// Install event - cache resources
self.addEventListener('install', (event) => {
  console.log('Service Worker installing...');
  event.waitUntil(
    caches.open(STATIC_CACHE)
      .then((cache) => {
        console.log('Opened cache');
        return cache.addAll(urlsToCache);
      })
      .catch((error) => {
        console.error('Cache installation failed:', error);
      })
  );
  self.skipWaiting();
});

// Fetch event - serve from cache when offline with network timeout
self.addEventListener('fetch', (event) => {
  // Skip non-GET requests and WebSocket connections
  if (event.request.method !== 'GET' || 
      event.request.url.includes('ws://') || 
      event.request.url.includes('wss://')) {
    return;
  }

  // Cache-first strategy for static assets
  if (isStaticAsset(event.request.url)) {
    event.respondWith(cacheFirst(event.request));
    return;
  }

  // Network-first with timeout for API requests
  if (event.request.url.includes('/api/')) {
    event.respondWith(networkFirstWithTimeout(event.request));
    return;
  }

  // Stale-while-revalidate for HTML pages
  event.respondWith(staleWhileRevalidate(event.request));
});

function isStaticAsset(url) {
  return url.includes('.js') || 
         url.includes('.css') || 
         url.includes('.png') || 
         url.includes('.jpg') || 
         url.includes('.svg') || 
         url.includes('manifest.json');
}

async function cacheFirst(request) {
  const cached = await caches.match(request);
  if (cached) {
    return cached;
  }
  
  try {
    const response = await fetch(request);
    if (response.status === 200) {
      const cache = await caches.open(STATIC_CACHE);
      cache.put(request, response.clone());
    }
    return response;
  } catch (error) {
    console.error('Cache first failed:', error);
    return new Response('Offline', { status: 503 });
  }
}

async function networkFirstWithTimeout(request) {
  try {
    const networkPromise = fetch(request);
    const timeoutPromise = new Promise((_, reject) => 
      setTimeout(() => reject(new Error('Network timeout')), NETWORK_TIMEOUT)
    );
    
    const response = await Promise.race([networkPromise, timeoutPromise]);
    
    if (response.status === 200) {
      const cache = await caches.open(DYNAMIC_CACHE);
      cache.put(request, response.clone());
    }
    return response;
  } catch (error) {
    console.log('Network failed, trying cache:', error);
    const cached = await caches.match(request);
    if (cached) {
      return cached;
    }
    return new Response(JSON.stringify({ error: 'Offline' }), {
      status: 503,
      headers: { 'Content-Type': 'application/json' }
    });
  }
}

async function staleWhileRevalidate(request) {
  const cached = await caches.match(request);
  
  const fetchPromise = fetch(request).then(response => {
    if (response.status === 200) {
      const cache = caches.open(DYNAMIC_CACHE);
      cache.then(c => c.put(request, response.clone()));
    }
    return response;
  }).catch(() => {
    // Return offline fallback for navigation requests
    if (request.mode === 'navigate') {
      return caches.match('/');
    }
    throw new Error('Network and cache failed');
  });

  return cached || fetchPromise;
}

// Activate event - clean up old caches
self.addEventListener('activate', (event) => {
  console.log('Service Worker activating...');
  event.waitUntil(
    caches.keys().then((cacheNames) => {
      return Promise.all(
        cacheNames.map((cacheName) => {
          if (cacheName !== STATIC_CACHE && 
              cacheName !== DYNAMIC_CACHE && 
              cacheName !== CACHE_NAME) {
            console.log('Deleting old cache:', cacheName);
            return caches.delete(cacheName);
          }
        })
      );
    }).then(() => {
      return self.clients.claim();
    })
  );
});

// Background sync for offline actions
self.addEventListener('sync', (event) => {
  if (event.tag === 'background-sync') {
    event.waitUntil(doBackgroundSync());
  }
});

async function doBackgroundSync() {
  // Handle background synchronization when connection is restored
  console.log('Background sync triggered');
}

// Push notifications support
self.addEventListener('push', (event) => {
  const options = {
    body: event.data ? event.data.text() : 'New message in video chat',
    icon: 'https://via.placeholder.com/192x192/667eea/ffffff?text=VC',
    badge: 'https://via.placeholder.com/72x72/667eea/ffffff?text=VC',
    vibrate: [100, 50, 100],
    data: {
      dateOfArrival: Date.now(),
      primaryKey: 1
    },
    actions: [
      {
        action: 'explore',
        title: 'Join Chat',
        icon: 'https://via.placeholder.com/128x128/667eea/ffffff?text=J'
      },
      {
        action: 'close',
        title: 'Dismiss',
        icon: 'https://via.placeholder.com/128x128/f44336/ffffff?text=X'
      }
    ]
  };

  event.waitUntil(
    self.registration.showNotification('WebRTC Video Chat', options)
  );
});

// Handle notification clicks
self.addEventListener('notificationclick', (event) => {
  event.notification.close();

  if (event.action === 'explore') {
    // Open the app when notification is clicked
    event.waitUntil(
      clients.matchAll().then((clientList) => {
        for (let i = 0; i < clientList.length; i++) {
          const client = clientList[i];
          if (client.url === '/' && 'focus' in client) {
            return client.focus();
          }
        }
        if (clients.openWindow) {
          return clients.openWindow('/');
        }
      })
    );
  }
});