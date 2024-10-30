package main

import (
	"context"
	"crypto/tls"
	"fmt"
	"golang.org/x/sync/semaphore"
	"io"
	"log"
	"net/http"
	"time"
)

type ClientWrapper struct {
	client    *http.Client
	semaphore *semaphore.Weighted
}

func NewClientWrapper(maxConcurrent int64) *ClientWrapper {
	customTransport := &http.Transport{
		TLSClientConfig: &tls.Config{
			InsecureSkipVerify: true,
		},
		MaxIdleConns:        int(maxConcurrent),
		MaxIdleConnsPerHost: int(maxConcurrent),
		IdleConnTimeout:     90 * time.Second,
		DisableCompression:  false, // Enable gzip compression
	}

	client := &http.Client{
		Transport: customTransport,
	}

	return &ClientWrapper{
		client:    client,
		semaphore: semaphore.NewWeighted(maxConcurrent),
	}
}

func (cw *ClientWrapper) Get(url string) error {
	err := cw.semaphore.Acquire(context.Background(), 1)
	if err != nil {
		return err
	}
	defer cw.semaphore.Release(1)

	for {
		startTime := time.Now()

		req, err := http.NewRequest("GET", url, nil)
		if err != nil {
			continue
		}

		req.Header.Set("Accept", "*/*")
		req.Header.Set("User-Agent", "go_client")

		resp, err := cw.client.Do(req)
		if err != nil {
			log.Printf("failed to connect to URL due to %s: %s", err, url)
			continue
		}

		_, err = io.Copy(io.Discard, resp.Body)
		resp.Body.Close()
		if err != nil {
			continue
		}

		log.Printf("scraped URL (status %d) in %v: %s",
			resp.StatusCode, time.Since(startTime), url)
		return nil
	}
}

func main() {
	startTime := time.Now()

	clientWrapper := NewClientWrapper(1000)

	// Create slice to hold all the channels for results
	results := make([]chan error, 1000)

	// Launch all requests
	for i := 0; i < 1000; i++ {
		results[i] = make(chan error, 1)
		url := fmt.Sprintf("https://127.0.0.1:3000/%d", i)

		go func(ch chan error, url string) {
			ch <- clientWrapper.Get(url)
		}(results[i], url)
	}

	// Wait for all results
	for i := 0; i < 1000; i++ {
		if err := <-results[i]; err != nil {
			log.Printf("Error: %v", err)
		}
	}

	log.Printf("total execution time: %v", time.Since(startTime))
}
