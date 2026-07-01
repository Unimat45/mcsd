package api

import (
	"encoding/json"
	"net/http"
	"path/filepath"
	"sort"
	"strconv"

	"github.com/mathieu/mcsd/internal/properties"
)

type propertiesResponse struct {
	Path   string            `json:"path"`
	Exists bool              `json:"exists"`
	Values map[string]string `json:"values"`
	Order  []string          `json:"order"`
	Schema []properties.Spec `json:"schema"`
}

type propertiesUpdate struct {
	Values map[string]string `json:"values"`
}

type eulaResponse struct {
	Path     string `json:"path"`
	Exists   bool   `json:"exists"`
	Accepted bool   `json:"accepted"`
}

type eulaUpdate struct {
	Accepted bool `json:"accepted"`
}

// serverFile returns the absolute path to a file inside a server's directory,
// plus the instance, or reports not-found.
func (s *Server) serverFile(w http.ResponseWriter, r *http.Request, name string) (string, bool) {
	inst, ok := s.sup.Get(r.PathValue("id"))
	if !ok {
		writeError(w, http.StatusNotFound, "server not found")
		return "", false
	}
	return filepath.Join(inst.Config().Directory, name), true
}

func (s *Server) getProperties(w http.ResponseWriter, r *http.Request) {
	path, ok := s.serverFile(w, r, "server.properties")
	if !ok {
		return
	}
	p, exists, err := properties.Load(path)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, propertiesResponse{
		Path:   path,
		Exists: exists,
		Values: p.Map(),
		Order:  p.Order(),
		Schema: properties.Schema,
	})
}

func (s *Server) putProperties(w http.ResponseWriter, r *http.Request) {
	path, ok := s.serverFile(w, r, "server.properties")
	if !ok {
		return
	}
	var update propertiesUpdate
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON: "+err.Error())
		return
	}

	p, _, err := properties.Load(path)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}

	// Apply updates deterministically: known keys in schema order first, then
	// any remaining keys sorted. This keeps freshly-created files tidy while
	// updates to existing keys stay in place.
	for _, spec := range properties.Schema {
		if v, present := update.Values[spec.Key]; present {
			p.Set(spec.Key, v)
		}
	}
	var extras []string
	for k := range update.Values {
		if _, known := properties.Lookup(k); !known {
			extras = append(extras, k)
		}
	}
	sort.Strings(extras)
	for _, k := range extras {
		p.Set(k, update.Values[k])
	}

	if err := properties.Save(path, p); err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, propertiesResponse{
		Path:   path,
		Exists: true,
		Values: p.Map(),
		Order:  p.Order(),
		Schema: properties.Schema,
	})
}

func (s *Server) getEula(w http.ResponseWriter, r *http.Request) {
	path, ok := s.serverFile(w, r, "eula.txt")
	if !ok {
		return
	}
	p, exists, err := properties.Load(path)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	v, _ := p.Get("eula")
	accepted, _ := strconv.ParseBool(v)
	writeJSON(w, http.StatusOK, eulaResponse{Path: path, Exists: exists, Accepted: accepted})
}

func (s *Server) putEula(w http.ResponseWriter, r *http.Request) {
	path, ok := s.serverFile(w, r, "eula.txt")
	if !ok {
		return
	}
	var update eulaUpdate
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON: "+err.Error())
		return
	}

	p, exists, err := properties.Load(path)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	if !exists {
		p = properties.Parse([]byte("#By changing eula to TRUE you agree to the Minecraft EULA (https://aka.ms/MinecraftEULA).\n"))
	}
	p.Set("eula", strconv.FormatBool(update.Accepted))
	if err := properties.Save(path, p); err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, eulaResponse{Path: path, Exists: true, Accepted: update.Accepted})
}
